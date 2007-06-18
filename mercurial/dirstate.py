"""
dirstate.py - working directory tracking for mercurial

Copyright 2005, 2006 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from node import *
from i18n import _
import struct, os, time, bisect, stat, strutil, util, re, errno
import cStringIO

class dirstate(object):
    format = ">cllll"

    def __init__(self, opener, ui, root):
        self.opener = opener
        self.root = root
        self.dirty = 0
        self.ui = ui
        self.ignorefunc = None
        self._slash = None

    def __getattr__(self, name):
        if name == 'map':
            self.read()
            return self.map
        elif name == 'copymap':
            self.read()
            return self.copymap
        elif name == '_branch':
            try:
                self._branch = self.opener("branch").read().strip()\
                               or "default"
            except IOError:
                self._branch = "default"
            return self._branch
        elif name == 'pl':
            self.pl = [nullid, nullid]
            try:
                st = self.opener("dirstate").read(40)
                if len(st) == 40:
                    self.pl = st[:20], st[20:40]
            except IOError, err:
                if err.errno != errno.ENOENT: raise
            return self.pl
        elif name == 'dirs':
            self.dirs = {}
            for f in self.map:
                self.updatedirs(f, 1)
            return self.dirs
        else:
            raise AttributeError, name

    def wjoin(self, f):
        return os.path.join(self.root, f)

    def getcwd(self):
        cwd = os.getcwd()
        if cwd == self.root: return ''
        # self.root ends with a path separator if self.root is '/' or 'C:\'
        rootsep = self.root
        if not rootsep.endswith(os.sep):
            rootsep += os.sep
        if cwd.startswith(rootsep):
            return cwd[len(rootsep):]
        else:
            # we're outside the repo. return an absolute path.
            return cwd

    def pathto(self, f, cwd=None):
        if cwd is None:
            cwd = self.getcwd()
        path = util.pathto(self.root, cwd, f)
        if self._slash is None:
            self._slash = self.ui.configbool('ui', 'slash') and os.sep != '/'
        if self._slash:
            path = path.replace(os.sep, '/')
        return path

    def hgignore(self):
        '''return the contents of .hgignore files as a list of patterns.

        the files parsed for patterns include:
        .hgignore in the repository root
        any additional files specified in the [ui] section of ~/.hgrc

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
                if not line.endswith('\n'):
                    line += '\n'
                escape = False
                for i in xrange(len(line)):
                    if escape: escape = False
                    elif line[i] == '\\': escape = True
                    elif line[i] == '#': break
                line = line[:i].rstrip()
                if line: yield line
        repoignore = self.wjoin('.hgignore')
        files = [repoignore]
        files.extend(self.ui.hgignorefiles())
        pats = {}
        for f in files:
            try:
                pats[f] = []
                fp = open(f)
                syntax = 'relre:'
                for line in parselines(fp):
                    if line.startswith('syntax:'):
                        s = line[7:].strip()
                        try:
                            syntax = syntaxes[s]
                        except KeyError:
                            self.ui.warn(_("%s: ignoring invalid "
                                           "syntax '%s'\n") % (f, s))
                        continue
                    pat = syntax + line
                    for s in syntaxes.values():
                        if line.startswith(s):
                            pat = line
                            break
                    pats[f].append(pat)
            except IOError, inst:
                if f != repoignore:
                    self.ui.warn(_("skipping unreadable ignore file"
                                   " '%s': %s\n") % (f, inst.strerror))
        return pats

    def ignore(self, fn):
        '''default match function used by dirstate and
        localrepository.  this honours the repository .hgignore file
        and any other files specified in the [ui] section of .hgrc.'''
        if not self.ignorefunc:
            ignore = self.hgignore()
            allpats = []
            [allpats.extend(patlist) for patlist in ignore.values()]
            if allpats:
                try:
                    files, self.ignorefunc, anypats = (
                        util.matcher(self.root, inc=allpats, src='.hgignore'))
                except util.Abort:
                    # Re-raise an exception where the src is the right file
                    for f, patlist in ignore.items():
                        files, self.ignorefunc, anypats = (
                            util.matcher(self.root, inc=patlist, src=f))
            else:
                self.ignorefunc = util.never
        return self.ignorefunc(fn)

    def __del__(self):
        if self.dirty:
            self.write()

    def __getitem__(self, key):
        return self.map[key]

    _unknown = ('?', 0, 0, 0)

    def get(self, key):
        try:
            return self[key]
        except KeyError:
            return self._unknown

    def __contains__(self, key):
        return key in self.map

    def parents(self):
        return self.pl

    def branch(self):
        return self._branch

    def markdirty(self):
        if not self.dirty:
            self.dirty = 1

    def setparents(self, p1, p2=nullid):
        self.markdirty()
        self.pl = p1, p2

    def setbranch(self, branch):
        self._branch = branch
        self.opener("branch", "w").write(branch + '\n')

    def state(self, key):
        try:
            return self[key][0]
        except KeyError:
            return "?"

    def parse(self, st):
        self.pl = [st[:20], st[20: 40]]

        # deref fields so they will be local in loop
        map = self.map
        copymap = self.copymap
        format = self.format
        unpack = struct.unpack

        pos = 40
        e_size = struct.calcsize(format)

        while pos < len(st):
            newpos = pos + e_size
            e = unpack(format, st[pos:newpos])
            l = e[4]
            pos = newpos
            newpos = pos + l
            f = st[pos:newpos]
            if '\0' in f:
                f, c = f.split('\0')
                copymap[f] = c
            map[f] = e[:4]
            pos = newpos

    def read(self):
        self.map = {}
        self.copymap = {}
        self.pl = [nullid, nullid]
        try:
            st = self.opener("dirstate").read()
            if st:
                self.parse(st)
        except IOError, err:
            if err.errno != errno.ENOENT: raise

    def reload(self):
        for a in "map copymap _branch pl dirs".split():
            if hasattr(self, a):
                self.__delattr__(a)
        self.ignorefunc = None

    def copy(self, source, dest):
        self.markdirty()
        self.copymap[dest] = source

    def copied(self, file):
        return self.copymap.get(file, None)

    def copies(self):
        return self.copymap

    def updatedirs(self, path, delta):
        for c in strutil.findall(path, '/'):
            pc = path[:c]
            self.dirs.setdefault(pc, 0)
            self.dirs[pc] += delta

    def checkinterfering(self, files):
        def prefixes(f):
            for c in strutil.rfindall(f, '/'):
                yield f[:c]
        seendirs = {}
        for f in files:
            # shadows
            if self.dirs.get(f):
                raise util.Abort(_('directory named %r already in dirstate') %
                                 f)
            for d in prefixes(f):
                if d in seendirs:
                    break
                if d in self.map:
                    raise util.Abort(_('file named %r already in dirstate') %
                                     d)
                seendirs[d] = True
            # disallowed
            if '\r' in f or '\n' in f:
                raise util.Abort(_("'\\n' and '\\r' disallowed in filenames"))

    def update(self, files, state, **kw):
        ''' current states:
        n  normal
        m  needs merging
        r  marked for removal
        a  marked for addition'''

        if not files: return
        self.markdirty()
        if state == "a":
            self.checkinterfering(files)
        for f in files:
            if state == "r":
                self.map[f] = ('r', 0, 0, 0)
                self.updatedirs(f, -1)
            else:
                if state == "a":
                    self.updatedirs(f, 1)
                s = os.lstat(self.wjoin(f))
                st_size = kw.get('st_size', s.st_size)
                st_mtime = kw.get('st_mtime', s.st_mtime)
                self.map[f] = (state, s.st_mode, st_size, st_mtime)
            if self.copymap.has_key(f):
                del self.copymap[f]

    def forget(self, files):
        if not files: return
        self.markdirty()
        for f in files:
            try:
                del self.map[f]
                self.updatedirs(f, -1)
            except KeyError:
                self.ui.warn(_("not in dirstate: %s!\n") % f)
                pass

    def clear(self):
        self.map = {}
        self.copymap = {}
        self.dirs = None
        self.markdirty()

    def rebuild(self, parent, files):
        self.clear()
        for f in files:
            if files.execf(f):
                self.map[f] = ('n', 0777, -1, 0)
            else:
                self.map[f] = ('n', 0666, -1, 0)
        self.pl = (parent, nullid)
        self.markdirty()

    def write(self):
        if not self.dirty:
            return
        cs = cStringIO.StringIO()
        cs.write("".join(self.pl))
        for f, e in self.map.iteritems():
            c = self.copied(f)
            if c:
                f = f + "\0" + c
            e = struct.pack(self.format, e[0], e[1], e[2], e[3], len(f))
            cs.write(e)
            cs.write(f)
        st = self.opener("dirstate", "w", atomictemp=True)
        st.write(cs.getvalue())
        st.rename()
        self.dirty = 0

    def filterfiles(self, files):
        ret = {}
        unknown = []

        for x in files:
            if x == '.':
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
            bs = bisect.bisect(b, "%s%s" % (x, '/'))
            while bs < blen:
                s = b[bs]
                if len(s) > len(x) and s.startswith(x):
                    ret[s] = self.map[s]
                else:
                    break
                bs += 1
        return ret

    def supported_type(self, f, st, verbose=False):
        if stat.S_ISREG(st.st_mode) or stat.S_ISLNK(st.st_mode):
            return True
        if verbose:
            kind = 'unknown'
            if stat.S_ISCHR(st.st_mode): kind = _('character device')
            elif stat.S_ISBLK(st.st_mode): kind = _('block device')
            elif stat.S_ISFIFO(st.st_mode): kind = _('fifo')
            elif stat.S_ISSOCK(st.st_mode): kind = _('socket')
            elif stat.S_ISDIR(st.st_mode): kind = _('directory')
            self.ui.warn(_('%s: unsupported file type (type is %s)\n')
                         % (self.pathto(f), kind))
        return False

    def walk(self, files=None, match=util.always, badmatch=None):
        # filter out the stat
        for src, f, st in self.statwalk(files, match, badmatch=badmatch):
            yield src, f

    def statwalk(self, files=None, match=util.always, ignored=False,
                 badmatch=None, directories=False):
        '''
        walk recursively through the directory tree, finding all files
        matched by the match function

        results are yielded in a tuple (src, filename, st), where src
        is one of:
        'f' the file was found in the directory tree
        'd' the file is a directory of the tree
        'm' the file was only in the dirstate and not in the tree
        'b' file was not found and matched badmatch

        and st is the stat result if the file was found in the directory.
        '''

        # walk all files by default
        if not files:
            files = ['.']
            dc = self.map.copy()
        else:
            files = util.unique(files)
            dc = self.filterfiles(files)

        def imatch(file_):
            if file_ not in dc and self.ignore(file_):
                return False
            return match(file_)

        ignore = self.ignore
        if ignored:
            imatch = match
            ignore = util.never

        # self.root may end with a path separator when self.root == '/'
        common_prefix_len = len(self.root)
        if not self.root.endswith(os.sep):
            common_prefix_len += 1
        # recursion free walker, faster than os.walk.
        def findfiles(s):
            work = [s]
            if directories:
                yield 'd', util.normpath(s[common_prefix_len:]), os.lstat(s)
            while work:
                top = work.pop()
                names = os.listdir(top)
                names.sort()
                # nd is the top of the repository dir tree
                nd = util.normpath(top[common_prefix_len:])
                if nd == '.':
                    nd = ''
                else:
                    # do not recurse into a repo contained in this
                    # one. use bisect to find .hg directory so speed
                    # is good on big directory.
                    hg = bisect.bisect_left(names, '.hg')
                    if hg < len(names) and names[hg] == '.hg':
                        if os.path.isdir(os.path.join(top, '.hg')):
                            continue
                for f in names:
                    np = util.pconvert(os.path.join(nd, f))
                    if seen(np):
                        continue
                    p = os.path.join(top, f)
                    # don't trip over symlinks
                    st = os.lstat(p)
                    if stat.S_ISDIR(st.st_mode):
                        if not ignore(np):
                            work.append(p)
                            if directories:
                                yield 'd', np, st
                        if imatch(np) and np in dc:
                            yield 'm', np, st
                    elif imatch(np):
                        if self.supported_type(np, st):
                            yield 'f', np, st
                        elif np in dc:
                            yield 'm', np, st

        known = {'.hg': 1}
        def seen(fn):
            if fn in known: return True
            known[fn] = 1

        # step one, find all files that match our criteria
        files.sort()
        for ff in files:
            nf = util.normpath(ff)
            f = self.wjoin(ff)
            try:
                st = os.lstat(f)
            except OSError, inst:
                found = False
                for fn in dc:
                    if nf == fn or (fn.startswith(nf) and fn[len(nf)] == '/'):
                        found = True
                        break
                if not found:
                    if inst.errno != errno.ENOENT or not badmatch:
                        self.ui.warn('%s: %s\n' % (self.pathto(ff),
                                                   inst.strerror))
                    elif badmatch and badmatch(ff) and imatch(nf):
                        yield 'b', ff, None
                continue
            if stat.S_ISDIR(st.st_mode):
                cmp1 = (lambda x, y: cmp(x[1], y[1]))
                sorted_ = [ x for x in findfiles(f) ]
                sorted_.sort(cmp1)
                for e in sorted_:
                    yield e
            else:
                if not seen(nf) and match(nf):
                    if self.supported_type(ff, st, verbose=True):
                        yield 'f', nf, st
                    elif ff in dc:
                        yield 'm', nf, st

        # step two run through anything left in the dc hash and yield
        # if we haven't already seen it
        ks = dc.keys()
        ks.sort()
        for k in ks:
            if not seen(k) and imatch(k):
                yield 'm', k, None

    def status(self, files=None, match=util.always, list_ignored=False,
               list_clean=False):
        lookup, modified, added, unknown, ignored = [], [], [], [], []
        removed, deleted, clean = [], [], []

        for src, fn, st in self.statwalk(files, match, ignored=list_ignored):
            try:
                type_, mode, size, time = self[fn]
            except KeyError:
                if list_ignored and self.ignore(fn):
                    ignored.append(fn)
                else:
                    unknown.append(fn)
                continue
            if src == 'm':
                nonexistent = True
                if not st:
                    try:
                        st = os.lstat(self.wjoin(fn))
                    except OSError, inst:
                        if inst.errno != errno.ENOENT:
                            raise
                        st = None
                    # We need to re-check that it is a valid file
                    if st and self.supported_type(fn, st):
                        nonexistent = False
                # XXX: what to do with file no longer present in the fs
                # who are not removed in the dirstate ?
                if nonexistent and type_ in "nm":
                    deleted.append(fn)
                    continue
            # check the common case first
            if type_ == 'n':
                if not st:
                    st = os.lstat(self.wjoin(fn))
                if size >= 0 and (size != st.st_size
                                  or (mode ^ st.st_mode) & 0100):
                    modified.append(fn)
                elif time != int(st.st_mtime):
                    lookup.append(fn)
                elif list_clean:
                    clean.append(fn)
            elif type_ == 'm':
                modified.append(fn)
            elif type_ == 'a':
                added.append(fn)
            elif type_ == 'r':
                removed.append(fn)

        return (lookup, modified, added, removed, deleted, unknown, ignored,
                clean)
