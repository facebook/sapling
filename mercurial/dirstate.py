"""
dirstate.py - working directory tracking for mercurial

Copyright 2005 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

import struct, os
from node import *
from demandload import *
demandload(globals(), "time bisect stat util re")

class dirstate:
    def __init__(self, opener, ui, root):
        self.opener = opener
        self.root = root
        self.dirty = 0
        self.ui = ui
        self.map = None
        self.pl = None
        self.copies = {}
        self.ignorefunc = None

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def getcwd(self):
        cwd = os.getcwd()
        if cwd == self.root: return ''
        return cwd[len(self.root) + 1:]

    def ignore(self, f):
        if not self.ignorefunc:
            bigpat = []
            try:
                l = file(self.wjoin(".hgignore"))
                for pat in l:
                    p = pat.rstrip()
                    if p:
                        try:
                            re.compile(p)
                        except:
                            self.ui.warn("ignoring invalid ignore"
                                         + " regular expression '%s'\n" % p)
                        else:
                            bigpat.append(p)
            except IOError: pass

            if bigpat:
                s = "(?:%s)" % (")|(?:".join(bigpat))
                r = re.compile(s)
                self.ignorefunc = r.search
            else:
                self.ignorefunc = util.never

        return self.ignorefunc(f)

    def __del__(self):
        if self.dirty:
            self.write()

    def __getitem__(self, key):
        try:
            return self.map[key]
        except TypeError:
            self.read()
            return self[key]

    def __contains__(self, key):
        if not self.map: self.read()
        return key in self.map

    def parents(self):
        if not self.pl:
            self.read()
        return self.pl

    def markdirty(self):
        if not self.dirty:
            self.dirty = 1

    def setparents(self, p1, p2=nullid):
        self.markdirty()
        self.pl = p1, p2

    def state(self, key):
        try:
            return self[key][0]
        except KeyError:
            return "?"

    def read(self):
        if self.map is not None: return self.map

        self.map = {}
        self.pl = [nullid, nullid]
        try:
            st = self.opener("dirstate").read()
            if not st: return
        except: return

        self.pl = [st[:20], st[20: 40]]

        pos = 40
        while pos < len(st):
            e = struct.unpack(">cllll", st[pos:pos+17])
            l = e[4]
            pos += 17
            f = st[pos:pos + l]
            if '\0' in f:
                f, c = f.split('\0')
                self.copies[f] = c
            self.map[f] = e[:4]
            pos += l

    def copy(self, source, dest):
        self.read()
        self.markdirty()
        self.copies[dest] = source

    def copied(self, file):
        return self.copies.get(file, None)

    def update(self, files, state, **kw):
        ''' current states:
        n  normal
        m  needs merging
        r  marked for removal
        a  marked for addition'''

        if not files: return
        self.read()
        self.markdirty()
        for f in files:
            if state == "r":
                self.map[f] = ('r', 0, 0, 0)
            else:
                s = os.stat(os.path.join(self.root, f))
                st_size = kw.get('st_size', s.st_size)
                st_mtime = kw.get('st_mtime', s.st_mtime)
                self.map[f] = (state, s.st_mode, st_size, st_mtime)

    def forget(self, files):
        if not files: return
        self.read()
        self.markdirty()
        for f in files:
            try:
                del self.map[f]
            except KeyError:
                self.ui.warn("not in dirstate: %s!\n" % f)
                pass

    def clear(self):
        self.map = {}
        self.markdirty()

    def write(self):
        st = self.opener("dirstate", "w")
        st.write("".join(self.pl))
        for f, e in self.map.items():
            c = self.copied(f)
            if c:
                f = f + "\0" + c
            e = struct.pack(">cllll", e[0], e[1], e[2], e[3], len(f))
            st.write(e + f)
        self.dirty = 0

    def filterfiles(self, files):
        ret = {}
        unknown = []

        for x in files:
            if x is '.':
                return self.map.copy()
            if x not in self.map:
                unknown.append(x)
            else:
                ret[x] = self.map[x]

        if not unknown:
            return ret

        b = self.map.keys()
        b.sort()
        blen = len(b)

        for x in unknown:
            bs = bisect.bisect(b, x)
            if bs != 0 and  b[bs-1] == x:
                ret[x] = self.map[x]
                continue
            while bs < blen:
                s = b[bs]
                if len(s) > len(x) and s.startswith(x) and s[len(x)] == '/':
                    ret[s] = self.map[s]
                else:
                    break
                bs += 1
        return ret

    def walk(self, files=None, match=util.always, dc=None):
        self.read()

        # walk all files by default
        if not files:
            files = [self.root]
            if not dc:
                dc = self.map.copy()
        elif not dc:
            dc = self.filterfiles(files)

        known = {'.hg': 1}
        def seen(fn):
            if fn in known: return True
            known[fn] = 1
        def traverse():
            for ff in util.unique(files):
                f = os.path.join(self.root, ff)
                try:
                    st = os.stat(f)
                except OSError, inst:
                    if ff not in dc: self.ui.warn('%s: %s\n' % (
                        util.pathto(self.getcwd(), ff),
                        inst.strerror))
                    continue
                if stat.S_ISDIR(st.st_mode):
                    for dir, subdirs, fl in os.walk(f):
                        d = dir[len(self.root) + 1:]
                        nd = util.normpath(d)
                        if nd == '.': nd = ''
                        if seen(nd):
                            subdirs[:] = []
                            continue
                        for sd in subdirs:
                            ds = os.path.join(nd, sd +'/')
                            if self.ignore(ds) or not match(ds):
                                subdirs.remove(sd)
                        subdirs.sort()
                        fl.sort()
                        for fn in fl:
                            fn = util.pconvert(os.path.join(d, fn))
                            yield 'f', fn
                elif stat.S_ISREG(st.st_mode):
                    yield 'f', ff
                else:
                    kind = 'unknown'
                    if stat.S_ISCHR(st.st_mode): kind = 'character device'
                    elif stat.S_ISBLK(st.st_mode): kind = 'block device'
                    elif stat.S_ISFIFO(st.st_mode): kind = 'fifo'
                    elif stat.S_ISLNK(st.st_mode): kind = 'symbolic link'
                    elif stat.S_ISSOCK(st.st_mode): kind = 'socket'
                    self.ui.warn('%s: unsupported file type (type is %s)\n' % (
                        util.pathto(self.getcwd(), ff),
                        kind))

            ks = dc.keys()
            ks.sort()
            for k in ks:
                yield 'm', k

        # yield only files that match: all in dirstate, others only if
        # not in .hgignore

        for src, fn in util.unique(traverse()):
            fn = util.normpath(fn)
            if seen(fn): continue
            if fn not in dc and self.ignore(fn):
                continue
            if match(fn):
                yield src, fn

    def changes(self, files=None, match=util.always):
        self.read()
        if not files:
            dc = self.map.copy()
        else:
            dc = self.filterfiles(files)
        lookup, modified, added, unknown = [], [], [], []
        removed, deleted = [], []

        for src, fn in self.walk(files, match, dc=dc):
            try:
                s = os.stat(os.path.join(self.root, fn))
            except OSError:
                continue
            if not stat.S_ISREG(s.st_mode):
                continue
            c = dc.get(fn)
            if c:
                del dc[fn]
                if c[0] == 'm':
                    modified.append(fn)
                elif c[0] == 'a':
                    added.append(fn)
                elif c[0] == 'r':
                    unknown.append(fn)
                elif c[2] != s.st_size or (c[1] ^ s.st_mode) & 0100:
                    modified.append(fn)
                elif c[3] != s.st_mtime:
                    lookup.append(fn)
            else:
                unknown.append(fn)

        for fn, c in [(fn, c) for fn, c in dc.items() if match(fn)]:
            if c[0] == 'r':
                removed.append(fn)
            else:
                deleted.append(fn)
        return (lookup, modified, added, removed + deleted, unknown)
