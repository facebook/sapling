"""
dirstate.py - working directory tracking for mercurial

Copyright 2005 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

import struct, os
from node import *
from i18n import gettext as _
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
        self.blockignore = False

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def getcwd(self):
        cwd = os.getcwd()
        if cwd == self.root: return ''
        return cwd[len(self.root) + 1:]

    def hgignore(self):
        '''return the contents of .hgignore as a list of patterns.

        trailing white space is dropped.
        the escape character is backslash.
        comments start with #.
        empty lines are skipped.

        lines can be of the following formats:

        syntax: regexp # defaults following lines to non-rooted regexps
        syntax: glob   # defaults following lines to non-rooted globs
        re:pattern     # non-rooted regular expression
        glob:pattern   # non-rooted glob
        pattern        # pattern of the current default type'''
        syntaxes = {'re': 'relre:', 'regexp': 'relre:', 'glob': 'relglob:'}
        def parselines(fp):
            for line in fp:
                escape = False
                for i in xrange(len(line)):
                    if escape: escape = False
                    elif line[i] == '\\': escape = True
                    elif line[i] == '#': break
                line = line[:i].rstrip()
                if line: yield line
        pats = []
        try:
            fp = open(self.wjoin('.hgignore'))
            syntax = 'relre:'
            for line in parselines(fp):
                if line.startswith('syntax:'):
                    s = line[7:].strip()
                    try:
                        syntax = syntaxes[s]
                    except KeyError:
                        self.ui.warn(_("ignoring invalid syntax '%s'\n") % s)
                    continue
                pat = syntax + line
                for s in syntaxes.values():
                    if line.startswith(s):
                        pat = line
                        break
                pats.append(pat)
        except IOError: pass
        return pats

    def ignore(self, fn):
        '''default match function used by dirstate and localrepository.
        this honours the .hgignore file, and nothing more.'''
        if self.blockignore:
            return False
        if not self.ignorefunc:
            ignore = self.hgignore()
            if ignore:
                files, self.ignorefunc, anypats = util.matcher(self.root,
                                                               inc=ignore)
            else:
                self.ignorefunc = util.never
        return self.ignorefunc(fn)

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
        if not self.pl:
            self.read()
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
                s = os.lstat(os.path.join(self.root, f))
                st_size = kw.get('st_size', s.st_size)
                st_mtime = kw.get('st_mtime', s.st_mtime)
                self.map[f] = (state, s.st_mode, st_size, st_mtime)
            if self.copies.has_key(f):
                del self.copies[f]

    def forget(self, files):
        if not files: return
        self.read()
        self.markdirty()
        for f in files:
            try:
                del self.map[f]
            except KeyError:
                self.ui.warn(_("not in dirstate: %s!\n") % f)
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

        def statmatch(file, stat):
            file = util.pconvert(file)
            if file not in dc and self.ignore(file):
                return False
            return match(file)

        return self.walkhelper(files=files, statmatch=statmatch, dc=dc)

    # walk recursively through the directory tree, finding all files
    # matched by the statmatch function
    #
    # results are yielded in a tuple (src, filename), where src is one of:
    # 'f' the file was found in the directory tree
    # 'm' the file was only in the dirstate and not in the tree
    #
    # dc is an optional arg for the current dirstate.  dc is not modified
    # directly by this function, but might be modified by your statmatch call.
    #
    def walkhelper(self, files, statmatch, dc):
        def supported_type(f, st):
            if stat.S_ISREG(st.st_mode):
                return True
            else:
                kind = 'unknown'
                if stat.S_ISCHR(st.st_mode): kind = _('character device')
                elif stat.S_ISBLK(st.st_mode): kind = _('block device')
                elif stat.S_ISFIFO(st.st_mode): kind = _('fifo')
                elif stat.S_ISLNK(st.st_mode): kind = _('symbolic link')
                elif stat.S_ISSOCK(st.st_mode): kind = _('socket')
                elif stat.S_ISDIR(st.st_mode): kind = _('directory')
                self.ui.warn(_('%s: unsupported file type (type is %s)\n') % (
                    util.pathto(self.getcwd(), f),
                    kind))
                return False

        # recursion free walker, faster than os.walk.
        def findfiles(s):
            retfiles = []
            work = [s]
            while work:
                top = work.pop()
                names = os.listdir(top)
                names.sort()
                # nd is the top of the repository dir tree
                nd = util.normpath(top[len(self.root) + 1:])
                if nd == '.': nd = ''
                for f in names:
                    np = os.path.join(nd, f)
                    if seen(np):
                        continue
                    p = os.path.join(top, f)
                    # don't trip over symlinks
                    st = os.lstat(p)
                    if stat.S_ISDIR(st.st_mode):
                        ds = os.path.join(nd, f +'/')
                        if statmatch(ds, st):
                            work.append(p)
                    elif statmatch(np, st) and supported_type(np, st):
                        yield util.pconvert(np)


        known = {'.hg': 1}
        def seen(fn):
            if fn in known: return True
            known[fn] = 1

        # step one, find all files that match our criteria
        files.sort()
        for ff in util.unique(files):
            f = os.path.join(self.root, ff)
            try:
                st = os.lstat(f)
            except OSError, inst:
                if ff not in dc: self.ui.warn('%s: %s\n' % (
                    util.pathto(self.getcwd(), ff),
                    inst.strerror))
                continue
            if stat.S_ISDIR(st.st_mode):
                sorted = [ x for x in findfiles(f) ]
                sorted.sort()
                for fl in sorted:
                    yield 'f', fl
            else:
                ff = util.normpath(ff)
                if seen(ff):
                    continue
                found = False
                self.blockignore = True
                if statmatch(ff, st) and supported_type(ff, st):
                    found = True
                self.blockignore = False
                if found:
                    yield 'f', ff

        # step two run through anything left in the dc hash and yield
        # if we haven't already seen it
        ks = dc.keys()
        ks.sort()
        for k in ks:
            if not seen(k) and (statmatch(k, None)):
                yield 'm', k

    def changes(self, files=None, match=util.always):
        self.read()
        if not files:
            files = [self.root]
            dc = self.map.copy()
        else:
            dc = self.filterfiles(files)
        lookup, modified, added, unknown = [], [], [], []
        removed, deleted = [], []

        # statmatch function to eliminate entries from the dirstate copy
        # and put files into the appropriate array.  This gets passed
        # to the walking code
        def statmatch(fn, s):
            fn = util.pconvert(fn)
            def checkappend(l, fn):
                if match is util.always or match(fn):
                    l.append(fn)

            if not s or stat.S_ISDIR(s.st_mode):
                if self.ignore(fn): return False
                return match(fn)

            c = dc.pop(fn, None)
            if c:
                type, mode, size, time = c
                # check the common case first
                if type == 'n':
                    if size != s.st_size or (mode ^ s.st_mode) & 0100:
                        checkappend(modified, fn)
                    elif time != s.st_mtime:
                        checkappend(lookup, fn)
                elif type == 'm':
                    checkappend(modified, fn)
                elif type == 'a':
                    checkappend(added, fn)
                elif type == 'r':
                    checkappend(unknown, fn)
            elif not self.ignore(fn) and match(fn):
                unknown.append(fn)
            # return false because we've already handled all cases above.
            # there's no need for the walking code to process the file
            # any further.
            return False

        # because our statmatch always returns false, self.walk will only
        # return files in the dirstate map that are not present in the FS.
        # But, we still need to iterate through the results to force the
        # walk to complete
        for src, fn in self.walkhelper(files, statmatch, dc):
            pass

        # there may be patterns in the .hgignore file that prevent us
        # from examining entire directories in the dirstate map, so we
        # go back and explicitly examine any matching files we've
        # ignored
        unexamined = [fn for fn in dc.iterkeys()
                      if self.ignore(fn) and match(fn)]

        for src, fn in self.walkhelper(unexamined, statmatch, dc):
            pass

        # anything left in dc didn't exist in the filesystem
        for fn, c in dc.iteritems():
            if not match(fn): continue
            if c[0] == 'r':
                removed.append(fn)
            else:
                deleted.append(fn)
        return (lookup, modified, added, removed + deleted, unknown)
