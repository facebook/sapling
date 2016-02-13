# manifest.py - manifest revision class for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import array
import heapq
import os
import struct

from .i18n import _
from . import (
    error,
    mdiff,
    parsers,
    revlog,
    util,
)

propertycache = util.propertycache

def _parsev1(data):
    # This method does a little bit of excessive-looking
    # precondition checking. This is so that the behavior of this
    # class exactly matches its C counterpart to try and help
    # prevent surprise breakage for anyone that develops against
    # the pure version.
    if data and data[-1] != '\n':
        raise ValueError('Manifest did not end in a newline.')
    prev = None
    for l in data.splitlines():
        if prev is not None and prev > l:
            raise ValueError('Manifest lines not in sorted order.')
        prev = l
        f, n = l.split('\0')
        if len(n) > 40:
            yield f, revlog.bin(n[:40]), n[40:]
        else:
            yield f, revlog.bin(n), ''

def _parsev2(data):
    metadataend = data.find('\n')
    # Just ignore metadata for now
    pos = metadataend + 1
    prevf = ''
    while pos < len(data):
        end = data.find('\n', pos + 1) # +1 to skip stem length byte
        if end == -1:
            raise ValueError('Manifest ended with incomplete file entry.')
        stemlen = ord(data[pos])
        items = data[pos + 1:end].split('\0')
        f = prevf[:stemlen] + items[0]
        if prevf > f:
            raise ValueError('Manifest entries not in sorted order.')
        fl = items[1]
        # Just ignore metadata (items[2:] for now)
        n = data[end + 1:end + 21]
        yield f, n, fl
        pos = end + 22
        prevf = f

def _parse(data):
    """Generates (path, node, flags) tuples from a manifest text"""
    if data.startswith('\0'):
        return iter(_parsev2(data))
    else:
        return iter(_parsev1(data))

def _text(it, usemanifestv2):
    """Given an iterator over (path, node, flags) tuples, returns a manifest
    text"""
    if usemanifestv2:
        return _textv2(it)
    else:
        return _textv1(it)

def _textv1(it):
    files = []
    lines = []
    _hex = revlog.hex
    for f, n, fl in it:
        files.append(f)
        # if this is changed to support newlines in filenames,
        # be sure to check the templates/ dir again (especially *-raw.tmpl)
        lines.append("%s\0%s%s\n" % (f, _hex(n), fl))

    _checkforbidden(files)
    return ''.join(lines)

def _textv2(it):
    files = []
    lines = ['\0\n']
    prevf = ''
    for f, n, fl in it:
        files.append(f)
        stem = os.path.commonprefix([prevf, f])
        stemlen = min(len(stem), 255)
        lines.append("%c%s\0%s\n%s\n" % (stemlen, f[stemlen:], fl, n))
        prevf = f
    _checkforbidden(files)
    return ''.join(lines)

class _lazymanifest(dict):
    """This is the pure implementation of lazymanifest.

    It has not been optimized *at all* and is not lazy.
    """

    def __init__(self, data):
        dict.__init__(self)
        for f, n, fl in _parse(data):
            self[f] = n, fl

    def __setitem__(self, k, v):
        node, flag = v
        assert node is not None
        if len(node) > 21:
            node = node[:21] # match c implementation behavior
        dict.__setitem__(self, k, (node, flag))

    def __iter__(self):
        return iter(sorted(dict.keys(self)))

    def iterkeys(self):
        return iter(sorted(dict.keys(self)))

    def iterentries(self):
        return ((f, e[0], e[1]) for f, e in sorted(self.iteritems()))

    def copy(self):
        c = _lazymanifest('')
        c.update(self)
        return c

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.'''
        diff = {}

        for fn, e1 in self.iteritems():
            if fn not in m2:
                diff[fn] = e1, (None, '')
            else:
                e2 = m2[fn]
                if e1 != e2:
                    diff[fn] = e1, e2
                elif clean:
                    diff[fn] = None

        for fn, e2 in m2.iteritems():
            if fn not in self:
                diff[fn] = (None, ''), e2

        return diff

    def filtercopy(self, filterfn):
        c = _lazymanifest('')
        for f, n, fl in self.iterentries():
            if filterfn(f):
                c[f] = n, fl
        return c

    def text(self):
        """Get the full data of this manifest as a bytestring."""
        return _textv1(self.iterentries())

try:
    _lazymanifest = parsers.lazymanifest
except AttributeError:
    pass

class manifestdict(object):
    def __init__(self, data=''):
        if data.startswith('\0'):
            #_lazymanifest can not parse v2
            self._lm = _lazymanifest('')
            for f, n, fl in _parsev2(data):
                self._lm[f] = n, fl
        else:
            self._lm = _lazymanifest(data)

    def __getitem__(self, key):
        return self._lm[key][0]

    def find(self, key):
        return self._lm[key]

    def __len__(self):
        return len(self._lm)

    def __setitem__(self, key, node):
        self._lm[key] = node, self.flags(key, '')

    def __contains__(self, key):
        return key in self._lm

    def __delitem__(self, key):
        del self._lm[key]

    def __iter__(self):
        return self._lm.__iter__()

    def iterkeys(self):
        return self._lm.iterkeys()

    def keys(self):
        return list(self.iterkeys())

    def filesnotin(self, m2):
        '''Set of files in this manifest that are not in the other'''
        files = set(self)
        files.difference_update(m2)
        return files

    @propertycache
    def _dirs(self):
        return util.dirs(self)

    def dirs(self):
        return self._dirs

    def hasdir(self, dir):
        return dir in self._dirs

    def _filesfastpath(self, match):
        '''Checks whether we can correctly and quickly iterate over matcher
        files instead of over manifest files.'''
        files = match.files()
        return (len(files) < 100 and (match.isexact() or
            (match.prefix() and all(fn in self for fn in files))))

    def walk(self, match):
        '''Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        '''
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

        # for dirstate.walk, files=['.'] means "walk the whole tree".
        # follow that here, too
        fset.discard('.')

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def matches(self, match):
        '''generate a new manifest filtered by the match argument'''
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

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.
          clean: if true, include files unchanged between these manifests
                 with a None value in the returned dictionary.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        '''
        return self._lm.diff(m2._lm, clean)

    def setflag(self, key, flag):
        self._lm[key] = self[key], flag

    def get(self, key, default=None):
        try:
            return self._lm[key][0]
        except KeyError:
            return default

    def flags(self, key, default=''):
        try:
            return self._lm[key][1]
        except KeyError:
            return default

    def copy(self):
        c = manifestdict()
        c._lm = self._lm.copy()
        return c

    def iteritems(self):
        return (x[:2] for x in self._lm.iterentries())

    def iterentries(self):
        return self._lm.iterentries()

    def text(self, usemanifestv2=False):
        if usemanifestv2:
            return _textv2(self._lm.iterentries())
        else:
            # use (probably) native version for v1
            return self._lm.text()

    def fastdelta(self, base, changes):
        """Given a base manifest text as an array.array and a list of changes
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
                        raise AssertionError(
                                _("failed to remove %s from manifest") % f)
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
            arraytext = array.array('c', self.text())
            deltatext = mdiff.textdiff(base, arraytext)

        return arraytext, deltatext

def _msearch(m, s, lo=0, hi=None):
    '''return a tuple (start, end) that says where to find s within m.

    If the string is found m[start:end] are the line containing
    that string.  If start == end the string was not found and
    they indicate the proper sorted insertion point.

    m should be a buffer or a string
    s is a string'''
    def advance(i, c):
        while i < lenm and m[i] != c:
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
        while start > 0 and m[start - 1] != '\n':
            start -= 1
        end = advance(start, '\0')
        if m[start:end] < s:
            # we know that after the null there are 40 bytes of sha1
            # this translates to the bisect lo = mid + 1
            lo = advance(end + 40, '\n') + 1
        else:
            # this translates to the bisect hi = mid
            hi = start
    end = advance(lo, '\0')
    found = m[lo:end]
    if s == found:
        # we know that after the null there are 40 bytes of sha1
        end = advance(end + 40, '\n')
        return (lo, end + 1)
    else:
        return (lo, lo)

def _checkforbidden(l):
    """Check filenames for illegal characters."""
    for f in l:
        if '\n' in f or '\r' in f:
            raise error.RevlogError(
                _("'\\n' and '\\r' disallowed in filenames: %r") % f)


# apply the changes collected during the bisect loop to our addlist
# return a delta suitable for addrevision
def _addlistdelta(addlist, x):
    # for large addlist arrays, building a new array is cheaper
    # than repeatedly modifying the existing one
    currentposition = 0
    newaddlist = array.array('c')

    for start, end, content in x:
        newaddlist += addlist[currentposition:start]
        if content:
            newaddlist += array.array('c', content)

        currentposition = end

    newaddlist += addlist[currentposition:]

    deltatext = "".join(struct.pack(">lll", start, end, len(content))
                   + content for start, end, content in x)
    return deltatext, newaddlist

def _splittopdir(f):
    if '/' in f:
        dir, subpath = f.split('/', 1)
        return dir + '/', subpath
    else:
        return '', f

_noop = lambda s: None

class treemanifest(object):
    def __init__(self, dir='', text=''):
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
                raise AssertionError('treemanifest constructor only accepts '
                                     'flat manifests')
            self.parse(text, readsubtree)
            self._dirty = True # Mark flat manifest dirty after parsing

    def _subpath(self, path):
        return self._dir + path

    def __len__(self):
        self._load()
        size = len(self._files)
        for m in self._dirs.values():
            size += m.__len__()
        return size

    def _isempty(self):
        self._load() # for consistency; already loaded by all callers
        return (not self._files and (not self._dirs or
                all(m._isempty() for m in self._dirs.values())))

    def __repr__(self):
        return ('<treemanifest dir=%s, node=%s, loaded=%s, dirty=%s at 0x%x>' %
                (self._dir, revlog.hex(self._node),
                 bool(self._loadfunc is _noop),
                 self._dirty, id(self)))

    def dir(self):
        '''The directory that this tree manifest represents, including a
        trailing '/'. Empty string for the repo root directory.'''
        return self._dir

    def node(self):
        '''This node of this instance. nullid for unsaved instances. Should
        be updated when the instance is read or written from a revlog.
        '''
        assert not self._dirty
        return self._node

    def setnode(self, node):
        self._node = node
        self._dirty = False

    def iterentries(self):
        self._load()
        for p, n in sorted(self._dirs.items() + self._files.items()):
            if p in self._files:
                yield self._subpath(p), n, self._flags.get(p, '')
            else:
                for x in n.iterentries():
                    yield x

    def iteritems(self):
        self._load()
        for p, n in sorted(self._dirs.items() + self._files.items()):
            if p in self._files:
                yield self._subpath(p), n
            else:
                for f, sn in n.iteritems():
                    yield f, sn

    def iterkeys(self):
        self._load()
        for p in sorted(self._dirs.keys() + self._files.keys()):
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
                return ''
            return self._dirs[dir].flags(subpath)
        else:
            if f in self._dirs:
                return ''
            return self._flags.get(f, '')

    def find(self, f):
        self._load()
        dir, subpath = _splittopdir(f)
        if dir:
            return self._dirs[dir].find(subpath)
        else:
            return self._files[f], self._flags.get(f, '')

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
            self._files[f] = n[:21] # to match manifestdict's behavior
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

    def filesnotin(self, m2):
        '''Set of files in this manifest that are not in the other'''
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
        return (dir + '/') in self._dirs

    def walk(self, match):
        '''Generates matching file names.

        Equivalent to manifest.matches(match).iterkeys(), but without creating
        an entirely new manifest.

        It also reports nonexistent files by marking them bad with match.bad().
        '''
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

        # for dirstate.walk, files=['.'] means "walk the whole tree".
        # follow that here, too
        fset.discard('.')

        for fn in sorted(fset):
            if not self.hasdir(fn):
                match.bad(fn, None)

    def _walk(self, match):
        '''Recursively generates matching file names for walk().'''
        if not match.visitdir(self._dir[:-1] or '.'):
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
        '''generate a new manifest filtered by the match argument'''
        if match.always():
            return self.copy()

        return self._matches(match)

    def _matches(self, match):
        '''recursively generate a new manifest filtered by the match argument.
        '''

        visit = match.visitdir(self._dir[:-1] or '.')
        if visit == 'all':
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

    def diff(self, m2, clean=False):
        '''Finds changes between the current manifest and m2.

        Args:
          m2: the manifest to which this manifest should be compared.
          clean: if true, include files unchanged between these manifests
                 with a None value in the returned dictionary.

        The result is returned as a dict with filename as key and
        values of the form ((n1,fl1),(n2,fl2)), where n1/n2 is the
        nodeid in the current/other manifest and fl1/fl2 is the flag
        in the current/other manifest. Where the file does not exist,
        the nodeid will be None and the flags will be the empty
        string.
        '''
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
                fl1 = t1._flags.get(fn, '')
                n2 = t2._files.get(fn, None)
                fl2 = t2._flags.get(fn, '')
                if n1 != n2 or fl1 != fl2:
                    result[t1._subpath(fn)] = ((n1, fl1), (n2, fl2))
                elif clean:
                    result[t1._subpath(fn)] = None

            for fn, n2 in t2._files.iteritems():
                if fn not in t1._files:
                    fl2 = t2._flags.get(fn, '')
                    result[t2._subpath(fn)] = ((None, ''), (n2, fl2))

        _diff(self, m2)
        return result

    def unmodifiedsince(self, m2):
        return not self._dirty and not m2._dirty and self._node == m2._node

    def parse(self, text, readsubtree):
        for f, n, fl in _parse(text):
            if fl == 't':
                f = f + '/'
                self._dirs[f] = readsubtree(self._subpath(f), n)
            elif '/' in f:
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

    def text(self, usemanifestv2=False):
        """Get the full data of this manifest as a bytestring."""
        self._load()
        return _text(self.iterentries(), usemanifestv2)

    def dirtext(self, usemanifestv2=False):
        """Get the full data of this directory as a bytestring. Make sure that
        any submanifests have been written first, so their nodeids are correct.
        """
        self._load()
        flags = self.flags
        dirs = [(d[:-1], self._dirs[d]._node, 't') for d in self._dirs]
        files = [(f, self._files[f], flags(f)) for f in self._files]
        return _text(sorted(dirs + files), usemanifestv2)

    def read(self, gettext, readsubtree):
        def _load_for_read(s):
            s.parse(gettext(), readsubtree)
            s._dirty = False
        self._loadfunc = _load_for_read

    def writesubtrees(self, m1, m2, writesubtree):
        self._load() # for consistency; should never have any effect here
        emptytree = treemanifest()
        for d, subm in self._dirs.iteritems():
            subp1 = m1._dirs.get(d, emptytree)._node
            subp2 = m2._dirs.get(d, emptytree)._node
            if subp1 == revlog.nullid:
                subp1, subp2 = subp2, subp1
            writesubtree(subm, subp1, subp2)

class manifest(revlog.revlog):
    def __init__(self, opener, dir='', dirlogcache=None):
        '''The 'dir' and 'dirlogcache' arguments are for internal use by
        manifest.manifest only. External users should create a root manifest
        log with manifest.manifest(opener) and call dirlog() on it.
        '''
        # During normal operations, we expect to deal with not more than four
        # revs at a time (such as during commit --amend). When rebasing large
        # stacks of commits, the number can go up, hence the config knob below.
        cachesize = 4
        usetreemanifest = False
        usemanifestv2 = False
        opts = getattr(opener, 'options', None)
        if opts is not None:
            cachesize = opts.get('manifestcachesize', cachesize)
            usetreemanifest = opts.get('treemanifest', usetreemanifest)
            usemanifestv2 = opts.get('manifestv2', usemanifestv2)
        self._mancache = util.lrucachedict(cachesize)
        self._treeinmem = usetreemanifest
        self._treeondisk = usetreemanifest
        self._usemanifestv2 = usemanifestv2
        indexfile = "00manifest.i"
        if dir:
            assert self._treeondisk
            if not dir.endswith('/'):
                dir = dir + '/'
            indexfile = "meta/" + dir + "00manifest.i"
        revlog.revlog.__init__(self, opener, indexfile)
        self._dir = dir
        # The dirlogcache is kept on the root manifest log
        if dir:
            self._dirlogcache = dirlogcache
        else:
            self._dirlogcache = {'': self}

    def _newmanifest(self, data=''):
        if self._treeinmem:
            return treemanifest(self._dir, data)
        return manifestdict(data)

    def dirlog(self, dir):
        if dir:
            assert self._treeondisk
        if dir not in self._dirlogcache:
            self._dirlogcache[dir] = manifest(self.opener, dir,
                                              self._dirlogcache)
        return self._dirlogcache[dir]

    def _slowreaddelta(self, node):
        r0 = self.deltaparent(self.rev(node))
        m0 = self.read(self.node(r0))
        m1 = self.read(node)
        md = self._newmanifest()
        for f, ((n0, fl0), (n1, fl1)) in m0.diff(m1).iteritems():
            if n1:
                md[f] = n1
                if fl1:
                    md.setflag(f, fl1)
        return md

    def readdelta(self, node):
        if self._usemanifestv2 or self._treeondisk:
            return self._slowreaddelta(node)
        r = self.rev(node)
        d = mdiff.patchtext(self.revdiff(self.deltaparent(r), r))
        return self._newmanifest(d)

    def readshallowdelta(self, node):
        '''For flat manifests, this is the same as readdelta(). For
        treemanifests, this will read the delta for this revlog's directory,
        without recursively reading subdirectory manifests. Instead, any
        subdirectory entry will be reported as it appears in the manifests, i.e.
        the subdirectory will be reported among files and distinguished only by
        its 't' flag.'''
        if not self._treeondisk:
            return self.readdelta(node)
        if self._usemanifestv2:
            raise error.Abort(
                "readshallowdelta() not implemented for manifestv2")
        r = self.rev(node)
        d = mdiff.patchtext(self.revdiff(self.deltaparent(r), r))
        return manifestdict(d)

    def readfast(self, node):
        '''use the faster of readdelta or read

        This will return a manifest which is either only the files
        added/modified relative to p1, or all files in the
        manifest. Which one is returned depends on the codepath used
        to retrieve the data.
        '''
        r = self.rev(node)
        deltaparent = self.deltaparent(r)
        if deltaparent != revlog.nullrev and deltaparent in self.parentrevs(r):
            return self.readdelta(node)
        return self.read(node)

    def readshallowfast(self, node):
        '''like readfast(), but calls readshallowdelta() instead of readdelta()
        '''
        r = self.rev(node)
        deltaparent = self.deltaparent(r)
        if deltaparent != revlog.nullrev and deltaparent in self.parentrevs(r):
            return self.readshallowdelta(node)
        return self.readshallow(node)

    def read(self, node):
        if node == revlog.nullid:
            return self._newmanifest() # don't upset local cache
        if node in self._mancache:
            return self._mancache[node][0]
        if self._treeondisk:
            def gettext():
                return self.revision(node)
            def readsubtree(dir, subm):
                return self.dirlog(dir).read(subm)
            m = self._newmanifest()
            m.read(gettext, readsubtree)
            m.setnode(node)
            arraytext = None
        else:
            text = self.revision(node)
            m = self._newmanifest(text)
            arraytext = array.array('c', text)
        self._mancache[node] = (m, arraytext)
        return m

    def readshallow(self, node):
        '''Reads the manifest in this directory. When using flat manifests,
        this manifest will generally have files in subdirectories in it. Does
        not cache the manifest as the callers generally do not read the same
        version twice.'''
        return manifestdict(self.revision(node))

    def find(self, node, f):
        '''look up entry for a single file efficiently.
        return (node, flags) pair if found, (None, None) if not.'''
        m = self.read(node)
        try:
            return m.find(f)
        except KeyError:
            return None, None

    def add(self, m, transaction, link, p1, p2, added, removed):
        if (p1 in self._mancache and not self._treeinmem
            and not self._usemanifestv2):
            # If our first parent is in the manifest cache, we can
            # compute a delta here using properties we know about the
            # manifest up-front, which may save time later for the
            # revlog layer.

            _checkforbidden(added)
            # combine the changed lists into one sorted iterator
            work = heapq.merge([(x, False) for x in added],
                               [(x, True) for x in removed])

            arraytext, deltatext = m.fastdelta(self._mancache[p1][1], work)
            cachedelta = self.rev(p1), deltatext
            text = util.buffer(arraytext)
            n = self.addrevision(text, transaction, link, p1, p2, cachedelta)
        else:
            # The first parent manifest isn't already loaded, so we'll
            # just encode a fulltext of the manifest and pass that
            # through to the revlog layer, and let it handle the delta
            # process.
            if self._treeondisk:
                m1 = self.read(p1)
                m2 = self.read(p2)
                n = self._addtree(m, transaction, link, m1, m2)
                arraytext = None
            else:
                text = m.text(self._usemanifestv2)
                n = self.addrevision(text, transaction, link, p1, p2)
                arraytext = array.array('c', text)

        self._mancache[n] = (m, arraytext)

        return n

    def _addtree(self, m, transaction, link, m1, m2):
        # If the manifest is unchanged compared to one parent,
        # don't write a new revision
        if m.unmodifiedsince(m1) or m.unmodifiedsince(m2):
            return m.node()
        def writesubtree(subm, subp1, subp2):
            sublog = self.dirlog(subm.dir())
            sublog.add(subm, transaction, link, subp1, subp2, None, None)
        m.writesubtrees(m1, m2, writesubtree)
        text = m.dirtext(self._usemanifestv2)
        # Double-check whether contents are unchanged to one parent
        if text == m1.dirtext(self._usemanifestv2):
            n = m1.node()
        elif text == m2.dirtext(self._usemanifestv2):
            n = m2.node()
        else:
            n = self.addrevision(text, transaction, link, m1.node(), m2.node())
        # Save nodeid so parent manifest can calculate its nodeid
        m.setnode(n)
        return n

    def clearcaches(self):
        super(manifest, self).clearcaches()
        self._mancache.clear()
        self._dirlogcache = {'': self}
