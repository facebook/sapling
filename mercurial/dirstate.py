"""
dirstate.py - working directory tracking for mercurial

Copyright 2005-2007 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from node import nullid
from i18n import _
import struct, os, bisect, stat, strutil, util, errno, ignore
import cStringIO, osutil, sys

_unknown = ('?', 0, 0, 0)
_format = ">cllll"

class dirstate(object):

    def __init__(self, opener, ui, root):
        self._opener = opener
        self._root = root
        self._dirty = False
        self._dirtypl = False
        self._ui = ui

    def __getattr__(self, name):
        if name == '_map':
            self._read()
            return self._map
        elif name == '_copymap':
            self._read()
            return self._copymap
        elif name == '_branch':
            try:
                self._branch = (self._opener("branch").read().strip()
                                or "default")
            except IOError:
                self._branch = "default"
            return self._branch
        elif name == '_pl':
            self._pl = [nullid, nullid]
            try:
                st = self._opener("dirstate").read(40)
                if len(st) == 40:
                    self._pl = st[:20], st[20:40]
            except IOError, err:
                if err.errno != errno.ENOENT: raise
            return self._pl
        elif name == '_dirs':
            self._dirs = {}
            for f in self._map:
                if self[f] != 'r':
                    self._incpath(f)
            return self._dirs
        elif name == '_ignore':
            files = [self._join('.hgignore')]
            for name, path in self._ui.configitems("ui"):
                if name == 'ignore' or name.startswith('ignore.'):
                    files.append(os.path.expanduser(path))
            self._ignore = ignore.ignore(self._root, files, self._ui.warn)
            return self._ignore
        elif name == '_slash':
            self._slash = self._ui.configbool('ui', 'slash') and os.sep != '/'
            return self._slash
        elif name == '_checkexec':
            self._checkexec = util.checkexec(self._root)
            return self._checkexec
        else:
            raise AttributeError, name

    def _join(self, f):
        return os.path.join(self._root, f)

    def getcwd(self):
        cwd = os.getcwd()
        if cwd == self._root: return ''
        # self._root ends with a path separator if self._root is '/' or 'C:\'
        rootsep = self._root
        if not util.endswithsep(rootsep):
            rootsep += os.sep
        if cwd.startswith(rootsep):
            return cwd[len(rootsep):]
        else:
            # we're outside the repo. return an absolute path.
            return cwd

    def pathto(self, f, cwd=None):
        if cwd is None:
            cwd = self.getcwd()
        path = util.pathto(self._root, cwd, f)
        if self._slash:
            return util.normpath(path)
        return path

    def __getitem__(self, key):
        ''' current states:
        n  normal
        m  needs merging
        r  marked for removal
        a  marked for addition
        ?  not tracked'''
        return self._map.get(key, ("?",))[0]

    def __contains__(self, key):
        return key in self._map

    def __iter__(self):
        a = self._map.keys()
        a.sort()
        for x in a:
            yield x

    def parents(self):
        return self._pl

    def branch(self):
        return self._branch

    def setparents(self, p1, p2=nullid):
        self._dirty = self._dirtypl = True
        self._pl = p1, p2

    def setbranch(self, branch):
        self._branch = branch
        self._opener("branch", "w").write(branch + '\n')

    def _read(self):
        self._map = {}
        self._copymap = {}
        if not self._dirtypl:
            self._pl = [nullid, nullid]
        try:
            st = self._opener("dirstate").read()
        except IOError, err:
            if err.errno != errno.ENOENT: raise
            return
        if not st:
            return

        if not self._dirtypl:
            self._pl = [st[:20], st[20: 40]]

        # deref fields so they will be local in loop
        dmap = self._map
        copymap = self._copymap
        unpack = struct.unpack
        e_size = struct.calcsize(_format)
        pos1 = 40
        l = len(st)

        # the inner loop
        while pos1 < l:
            pos2 = pos1 + e_size
            e = unpack(">cllll", st[pos1:pos2]) # a literal here is faster
            pos1 = pos2 + e[4]
            f = st[pos2:pos1]
            if '\0' in f:
                f, c = f.split('\0')
                copymap[f] = c
            dmap[f] = e # we hold onto e[4] because making a subtuple is slow

    def invalidate(self):
        for a in "_map _copymap _branch _pl _dirs _ignore".split():
            if a in self.__dict__:
                delattr(self, a)
        self._dirty = False

    def copy(self, source, dest):
        self._dirty = True
        self._copymap[dest] = source

    def copied(self, file):
        return self._copymap.get(file, None)

    def copies(self):
        return self._copymap

    def _incpath(self, path):
        c = path.rfind('/')
        if c >= 0:
            dirs = self._dirs
            base = path[:c]
            if base not in dirs:
                self._incpath(base)
                dirs[base] = 1
            else:
                dirs[base] += 1

    def _decpath(self, path):
        c = path.rfind('/')
        if c >= 0:
            base = path[:c]
            dirs = self._dirs
            if dirs[base] == 1:
                del dirs[base]
                self._decpath(base)
            else:
                dirs[base] -= 1

    def _incpathcheck(self, f):
        if '\r' in f or '\n' in f:
            raise util.Abort(_("'\\n' and '\\r' disallowed in filenames: %r")
                             % f)
        # shadows
        if f in self._dirs:
            raise util.Abort(_('directory %r already in dirstate') % f)
        for c in strutil.rfindall(f, '/'):
            d = f[:c]
            if d in self._dirs:
                break
            if d in self._map and self[d] != 'r':
                raise util.Abort(_('file %r in dirstate clashes with %r') %
                                 (d, f))
        self._incpath(f)

    def _changepath(self, f, newstate, relaxed=False):
        # handle upcoming path changes
        oldstate = self[f]
        if oldstate not in "?r" and newstate in "?r":
            if "_dirs" in self.__dict__:
                self._decpath(f)
            return
        if oldstate in "?r" and newstate not in "?r":
            if relaxed and oldstate == '?':
                # XXX
                # in relaxed mode we assume the caller knows
                # what it is doing, workaround for updating
                # dir-to-file revisions
                if "_dirs" in self.__dict__:
                    self._incpath(f)
                return
            self._incpathcheck(f)
            return

    def normal(self, f):
        'mark a file normal and clean'
        self._dirty = True
        self._changepath(f, 'n', True)
        s = os.lstat(self._join(f))
        self._map[f] = ('n', s.st_mode, s.st_size, s.st_mtime, 0)
        if f in self._copymap:
            del self._copymap[f]

    def normallookup(self, f):
        'mark a file normal, but possibly dirty'
        if self._pl[1] != nullid and f in self._map:
            # if there is a merge going on and the file was either
            # in state 'm' or dirty before being removed, restore that state.
            entry = self._map[f]
            if entry[0] == 'r' and entry[2] in (-1, -2):
                source = self._copymap.get(f)
                if entry[2] == -1:
                    self.merge(f)
                elif entry[2] == -2:
                    self.normaldirty(f)
                if source:
                    self.copy(source, f)
                return
            if entry[0] == 'm' or entry[0] == 'n' and entry[2] == -2:
                return
        self._dirty = True
        self._changepath(f, 'n', True)
        self._map[f] = ('n', 0, -1, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def normaldirty(self, f):
        'mark a file normal, but dirty'
        self._dirty = True
        self._changepath(f, 'n', True)
        self._map[f] = ('n', 0, -2, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def add(self, f):
        'mark a file added'
        self._dirty = True
        self._changepath(f, 'a')
        self._map[f] = ('a', 0, -1, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def remove(self, f):
        'mark a file removed'
        self._dirty = True
        self._changepath(f, 'r')
        size = 0
        if self._pl[1] != nullid and f in self._map:
            entry = self._map[f]
            if entry[0] == 'm':
                size = -1
            elif entry[0] == 'n' and entry[2] == -2:
                size = -2
        self._map[f] = ('r', 0, size, 0, 0)
        if size == 0 and f in self._copymap:
            del self._copymap[f]

    def merge(self, f):
        'mark a file merged'
        self._dirty = True
        s = os.lstat(self._join(f))
        self._changepath(f, 'm', True)
        self._map[f] = ('m', s.st_mode, s.st_size, s.st_mtime, 0)
        if f in self._copymap:
            del self._copymap[f]

    def forget(self, f):
        'forget a file'
        self._dirty = True
        try:
            self._changepath(f, '?')
            del self._map[f]
        except KeyError:
            self._ui.warn(_("not in dirstate: %s\n") % f)

    def clear(self):
        self._map = {}
        if "_dirs" in self.__dict__:
            delattr(self, "_dirs");
        self._copymap = {}
        self._pl = [nullid, nullid]
        self._dirty = True

    def rebuild(self, parent, files):
        self.clear()
        for f in files:
            if files.execf(f):
                self._map[f] = ('n', 0777, -1, 0, 0)
            else:
                self._map[f] = ('n', 0666, -1, 0, 0)
        self._pl = (parent, nullid)
        self._dirty = True

    def write(self):
        if not self._dirty:
            return
        st = self._opener("dirstate", "w", atomictemp=True)

        try:
            gran = int(self._ui.config('dirstate', 'granularity', 1))
        except ValueError:
            gran = 1
        limit = sys.maxint
        if gran > 0:
            limit = util.fstat(st).st_mtime - gran

        cs = cStringIO.StringIO()
        copymap = self._copymap
        pack = struct.pack
        write = cs.write
        write("".join(self._pl))
        for f, e in self._map.iteritems():
            if f in copymap:
                f = "%s\0%s" % (f, copymap[f])
            if e[3] > limit and e[0] == 'n':
                e = (e[0], 0, -1, -1, 0)
            e = pack(_format, e[0], e[1], e[2], e[3], len(f))
            write(e)
            write(f)
        st.write(cs.getvalue())
        st.rename()
        self._dirty = self._dirtypl = False

    def _filter(self, files):
        ret = {}
        unknown = []

        for x in files:
            if x == '.':
                return self._map.copy()
            if x not in self._map:
                unknown.append(x)
            else:
                ret[x] = self._map[x]

        if not unknown:
            return ret

        b = self._map.keys()
        b.sort()
        blen = len(b)

        for x in unknown:
            bs = bisect.bisect(b, "%s%s" % (x, '/'))
            while bs < blen:
                s = b[bs]
                if len(s) > len(x) and s.startswith(x):
                    ret[s] = self._map[s]
                else:
                    break
                bs += 1
        return ret

    def _supported(self, f, mode, verbose=False):
        if stat.S_ISREG(mode) or stat.S_ISLNK(mode):
            return True
        if verbose:
            kind = 'unknown'
            if stat.S_ISCHR(mode): kind = _('character device')
            elif stat.S_ISBLK(mode): kind = _('block device')
            elif stat.S_ISFIFO(mode): kind = _('fifo')
            elif stat.S_ISSOCK(mode): kind = _('socket')
            elif stat.S_ISDIR(mode): kind = _('directory')
            self._ui.warn(_('%s: unsupported file type (type is %s)\n')
                          % (self.pathto(f), kind))
        return False

    def _dirignore(self, f):
        if f == '.':
            return False
        if self._ignore(f):
            return True
        for c in strutil.findall(f, '/'):
            if self._ignore(f[:c]):
                return True
        return False

    def walk(self, files=None, match=util.always, badmatch=None):
        # filter out the stat
        for src, f, st in self.statwalk(files, match, badmatch=badmatch):
            yield src, f

    def statwalk(self, files=None, match=util.always, unknown=True,
                 ignored=False, badmatch=None, directories=False):
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
            dc = self._map.copy()
        else:
            files = util.unique(files)
            dc = self._filter(files)

        def imatch(file_):
            if file_ not in dc and self._ignore(file_):
                return False
            return match(file_)

        # TODO: don't walk unknown directories if unknown and ignored are False
        ignore = self._ignore
        dirignore = self._dirignore
        if ignored:
            imatch = match
            ignore = util.never
            dirignore = util.never

        # self._root may end with a path separator when self._root == '/'
        common_prefix_len = len(self._root)
        if not util.endswithsep(self._root):
            common_prefix_len += 1

        normpath = util.normpath
        listdir = osutil.listdir
        lstat = os.lstat
        bisect_left = bisect.bisect_left
        isdir = os.path.isdir
        pconvert = util.pconvert
        join = os.path.join
        s_isdir = stat.S_ISDIR
        supported = self._supported
        _join = self._join
        known = {'.hg': 1}

        # recursion free walker, faster than os.walk.
        def findfiles(s):
            work = [s]
            wadd = work.append
            found = []
            add = found.append
            if directories:
                add((normpath(s[common_prefix_len:]), 'd', lstat(s)))
            while work:
                top = work.pop()
                entries = listdir(top, stat=True)
                # nd is the top of the repository dir tree
                nd = normpath(top[common_prefix_len:])
                if nd == '.':
                    nd = ''
                else:
                    # do not recurse into a repo contained in this
                    # one. use bisect to find .hg directory so speed
                    # is good on big directory.
                    names = [e[0] for e in entries]
                    hg = bisect_left(names, '.hg')
                    if hg < len(names) and names[hg] == '.hg':
                        if isdir(join(top, '.hg')):
                            continue
                for f, kind, st in entries:
                    np = pconvert(join(nd, f))
                    if np in known:
                        continue
                    known[np] = 1
                    p = join(top, f)
                    # don't trip over symlinks
                    if kind == stat.S_IFDIR:
                        if not ignore(np):
                            wadd(p)
                            if directories:
                                add((np, 'd', st))
                        if np in dc and match(np):
                            add((np, 'm', st))
                    elif imatch(np):
                        if supported(np, st.st_mode):
                            add((np, 'f', st))
                        elif np in dc:
                            add((np, 'm', st))
            found.sort()
            return found

        # step one, find all files that match our criteria
        files.sort()
        for ff in files:
            nf = normpath(ff)
            f = _join(ff)
            try:
                st = lstat(f)
            except OSError, inst:
                found = False
                for fn in dc:
                    if nf == fn or (fn.startswith(nf) and fn[len(nf)] == '/'):
                        found = True
                        break
                if not found:
                    if inst.errno != errno.ENOENT or not badmatch:
                        self._ui.warn('%s: %s\n' %
                                      (self.pathto(ff), inst.strerror))
                    elif badmatch and badmatch(ff) and imatch(nf):
                        yield 'b', ff, None
                continue
            if s_isdir(st.st_mode):
                if not dirignore(nf):
                    for f, src, st in findfiles(f):
                        yield src, f, st
            else:
                if nf in known:
                    continue
                known[nf] = 1
                if match(nf):
                    if supported(ff, st.st_mode, verbose=True):
                        yield 'f', nf, st
                    elif ff in dc:
                        yield 'm', nf, st

        # step two run through anything left in the dc hash and yield
        # if we haven't already seen it
        ks = dc.keys()
        ks.sort()
        for k in ks:
            if k in known:
                continue
            known[k] = 1
            if imatch(k):
                yield 'm', k, None

    def status(self, files, match, list_ignored, list_clean, list_unknown=True):
        lookup, modified, added, unknown, ignored = [], [], [], [], []
        removed, deleted, clean = [], [], []

        files = files or []
        _join = self._join
        lstat = os.lstat
        cmap = self._copymap
        dmap = self._map
        ladd = lookup.append
        madd = modified.append
        aadd = added.append
        uadd = unknown.append
        iadd = ignored.append
        radd = removed.append
        dadd = deleted.append
        cadd = clean.append

        for src, fn, st in self.statwalk(files, match, unknown=list_unknown,
                                         ignored=list_ignored):
            if fn in dmap:
                type_, mode, size, time, foo = dmap[fn]
            else:
                if (list_ignored or fn in files) and self._dirignore(fn):
                    if list_ignored:
                        iadd(fn)
                elif list_unknown:
                    uadd(fn)
                continue
            if src == 'm':
                nonexistent = True
                if not st:
                    try:
                        st = lstat(_join(fn))
                    except OSError, inst:
                        if inst.errno not in (errno.ENOENT, errno.ENOTDIR):
                            raise
                        st = None
                    # We need to re-check that it is a valid file
                    if st and self._supported(fn, st.st_mode):
                        nonexistent = False
                # XXX: what to do with file no longer present in the fs
                # who are not removed in the dirstate ?
                if nonexistent and type_ in "nma":
                    dadd(fn)
                    continue
            # check the common case first
            if type_ == 'n':
                if not st:
                    st = lstat(_join(fn))
                if (size >= 0 and
                    (size != st.st_size
                     or ((mode ^ st.st_mode) & 0100 and self._checkexec))
                    or size == -2
                    or fn in self._copymap):
                    madd(fn)
                elif time != int(st.st_mtime):
                    ladd(fn)
                elif list_clean:
                    cadd(fn)
            elif type_ == 'm':
                madd(fn)
            elif type_ == 'a':
                aadd(fn)
            elif type_ == 'r':
                radd(fn)

        return (lookup, modified, added, removed, deleted, unknown, ignored,
                clean)
