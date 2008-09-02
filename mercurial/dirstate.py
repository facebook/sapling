"""
dirstate.py - working directory tracking for mercurial

Copyright 2005-2007 Matt Mackall <mpm@selenic.com>

This software may be used and distributed according to the terms
of the GNU General Public License, incorporated herein by reference.
"""

from node import nullid
from i18n import _
import struct, os, bisect, stat, util, errno, ignore
import cStringIO, osutil, sys

_unknown = ('?', 0, 0, 0)
_format = ">cllll"

def _finddirs(path):
    pos = len(path)
    while 1:
        pos = path.rfind('/', 0, pos)
        if pos == -1:
            break
        yield path[:pos]

class dirstate(object):

    def __init__(self, opener, ui, root):
        self._opener = opener
        self._root = root
        self._rootdir = os.path.join(root, '')
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
        elif name == '_foldmap':
            _foldmap = {}
            for name in self._map:
                norm = os.path.normcase(os.path.normpath(name))
                _foldmap[norm] = name
            self._foldmap = _foldmap
            return self._foldmap
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
            dirs = {}
            for f,s in self._map.items():
                if s[0] != 'r':
                    for base in _finddirs(f):
                        dirs[base] = dirs.get(base, 0) + 1
            self._dirs = dirs
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
        elif name == '_checklink':
            self._checklink = util.checklink(self._root)
            return self._checklink
        elif name == '_checkexec':
            self._checkexec = util.checkexec(self._root)
            return self._checkexec
        elif name == '_checkcase':
            self._checkcase = not util.checkcase(self._join('.hg'))
            return self._checkcase
        elif name == 'normalize':
            if self._checkcase:
                self.normalize = self._normalize
            else:
                self.normalize = lambda x: x
            return self.normalize
        else:
            raise AttributeError, name

    def _join(self, f):
        # much faster than os.path.join()
        # it's safe because f is always a relative path
        return self._rootdir + f

    def flagfunc(self, fallback):
        if self._checklink:
            if self._checkexec:
                def f(x):
                    p = self._join(x)
                    if os.path.islink(p):
                        return 'l'
                    if util.is_exec(p):
                        return 'x'
                    return ''
                return f
            def f(x):
                if os.path.islink(self._join(x)):
                    return 'l'
                if 'x' in fallback(x):
                    return 'x'
                return ''
            return f
        if self._checkexec:
            def f(x):
                if 'l' in fallback(x):
                    return 'l'
                if util.is_exec(self._join(x)):
                    return 'x'
                return ''
            return f
        return fallback

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
        for x in util.sort(self._map):
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
        for a in "_map _copymap _foldmap _branch _pl _dirs _ignore".split():
            if a in self.__dict__:
                delattr(self, a)
        self._dirty = False

    def copy(self, source, dest):
        if source == dest:
            return
        self._dirty = True
        self._copymap[dest] = source

    def copied(self, file):
        return self._copymap.get(file, None)

    def copies(self):
        return self._copymap

    def _droppath(self, f):
        if self[f] not in "?r" and "_dirs" in self.__dict__:
            dirs = self._dirs
            for base in _finddirs(f):
                if dirs[base] == 1:
                    del dirs[base]
                else:
                    dirs[base] -= 1

    def _addpath(self, f, check=False):
        oldstate = self[f]
        if check or oldstate == "r":
            if '\r' in f or '\n' in f:
                raise util.Abort(
                    _("'\\n' and '\\r' disallowed in filenames: %r") % f)
            if f in self._dirs:
                raise util.Abort(_('directory %r already in dirstate') % f)
            # shadows
            for d in _finddirs(f):
                if d in self._dirs:
                    break
                if d in self._map and self[d] != 'r':
                    raise util.Abort(
                        _('file %r in dirstate clashes with %r') % (d, f))
        if oldstate in "?r" and "_dirs" in self.__dict__:
            dirs = self._dirs
            for base in _finddirs(f):
                dirs[base] = dirs.get(base, 0) + 1

    def normal(self, f):
        'mark a file normal and clean'
        self._dirty = True
        self._addpath(f)
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
        self._addpath(f)
        self._map[f] = ('n', 0, -1, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def normaldirty(self, f):
        'mark a file normal, but dirty'
        self._dirty = True
        self._addpath(f)
        self._map[f] = ('n', 0, -2, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def add(self, f):
        'mark a file added'
        self._dirty = True
        self._addpath(f, True)
        self._map[f] = ('a', 0, -1, -1, 0)
        if f in self._copymap:
            del self._copymap[f]

    def remove(self, f):
        'mark a file removed'
        self._dirty = True
        self._droppath(f)
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
        self._addpath(f)
        self._map[f] = ('m', s.st_mode, s.st_size, s.st_mtime, 0)
        if f in self._copymap:
            del self._copymap[f]

    def forget(self, f):
        'forget a file'
        self._dirty = True
        try:
            self._droppath(f)
            del self._map[f]
        except KeyError:
            self._ui.warn(_("not in dirstate: %s\n") % f)

    def _normalize(self, path):
        if path not in self._foldmap:
            if not os.path.exists(path):
                return path
            self._foldmap[path] = util.fspath(path, self._root)
        return self._foldmap[path]

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
            if 'x' in files.flags(f):
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

    def _dirignore(self, f):
        if f == '.':
            return False
        if self._ignore(f):
            return True
        for p in _finddirs(f):
            if self._ignore(p):
                return True
        return False

    def walk(self, match, unknown, ignored):
        '''
        walk recursively through the directory tree, finding all files
        matched by the match function

        results are yielded in a tuple (filename, stat), where stat
        and st is the stat result if the file was found in the directory.
        '''

        def fwarn(f, msg):
            self._ui.warn('%s: %s\n' % (self.pathto(ff), msg))
            return False
        badfn = fwarn
        if hasattr(match, 'bad'):
            badfn = match.bad

        def badtype(f, mode):
            kind = 'unknown'
            if stat.S_ISCHR(mode): kind = _('character device')
            elif stat.S_ISBLK(mode): kind = _('block device')
            elif stat.S_ISFIFO(mode): kind = _('fifo')
            elif stat.S_ISSOCK(mode): kind = _('socket')
            elif stat.S_ISDIR(mode): kind = _('directory')
            self._ui.warn(_('%s: unsupported file type (type is %s)\n')
                          % (self.pathto(f), kind))

        ignore = self._ignore
        dirignore = self._dirignore
        if ignored:
            ignore = util.never
            dirignore = util.never
        elif not unknown:
            # if unknown and ignored are False, skip step 2
            ignore = util.always
            dirignore = util.always

        matchfn = match.matchfn
        dmap = self._map
        normpath = util.normpath
        normalize = self.normalize
        listdir = osutil.listdir
        lstat = os.lstat
        bisect_left = bisect.bisect_left
        pconvert = util.pconvert
        getkind = stat.S_IFMT
        dirkind = stat.S_IFDIR
        regkind = stat.S_IFREG
        lnkkind = stat.S_IFLNK
        join = self._join
        work = []
        wadd = work.append

        files = util.unique(match.files())
        if not files or '.' in files:
            files = ['']
        results = {'.hg': None}

        # step 1: find all explicit files
        for ff in util.sort(files):
            nf = normalize(normpath(ff))
            if nf in results:
                continue

            try:
                st = lstat(join(nf))
                kind = getkind(st.st_mode)
                if kind == dirkind:
                    if not dirignore(nf):
                        wadd(nf)
                elif kind == regkind or kind == lnkkind:
                    results[nf] = st
                else:
                    badtype(ff, kind)
                    if nf in dmap:
                        results[nf] = None
            except OSError, inst:
                keep = False
                prefix = nf + "/"
                for fn in dmap:
                    if nf == fn or fn.startswith(prefix):
                        keep = True
                        break
                if not keep:
                    if inst.errno != errno.ENOENT:
                        fwarn(ff, inst.strerror)
                    elif badfn(ff, inst.strerror):
                        if (nf in dmap or not ignore(nf)) and matchfn(nf):
                            results[nf] = None

        # step 2: visit subdirectories
        while work:
            nd = work.pop()
            if hasattr(match, 'dir'):
                match.dir(nd)
            entries = listdir(join(nd), stat=True)
            if nd == '.':
                nd = ''
            else:
                # do not recurse into a repo contained in this
                # one. use bisect to find .hg directory so speed
                # is good on big directory.
                hg = bisect_left(entries, ('.hg'))
                if hg < len(entries) and entries[hg][0] == '.hg' \
                        and entries[hg][1] == dirkind:
                        continue
            for f, kind, st in entries:
                nf = normalize(nd and (nd + "/" + f) or f)
                if nf not in results:
                    if kind == dirkind:
                        if not ignore(nf):
                            wadd(nf)
                        if nf in dmap and matchfn(nf):
                            results[nf] = None
                    elif kind == regkind or kind == lnkkind:
                        if nf in dmap:
                            if matchfn(nf):
                                results[nf] = st
                        elif matchfn(nf) and not ignore(nf):
                            results[nf] = st
                    elif nf in dmap and matchfn(nf):
                        results[nf] = None

        # step 3: report unseen items in the dmap hash
        visit = [f for f in dmap if f not in results and match(f)]
        for nf in util.sort(visit):
            results[nf] = None
            try:
                st = lstat(join(nf))
                kind = getkind(st.st_mode)
                if kind == regkind or kind == lnkkind:
                    results[nf] = st
            except OSError, inst:
                if inst.errno not in (errno.ENOENT, errno.ENOTDIR):
                    raise

        del results['.hg']
        return results

    def status(self, match, ignored, clean, unknown):
        listignored, listclean, listunknown = ignored, clean, unknown
        lookup, modified, added, unknown, ignored = [], [], [], [], []
        removed, deleted, clean = [], [], []

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

        for fn, st in self.walk(match, listunknown, listignored).iteritems():
            if fn not in dmap:
                if (listignored or match.exact(fn)) and self._dirignore(fn):
                    if listignored:
                        iadd(fn)
                elif listunknown:
                    uadd(fn)
                continue

            state, mode, size, time, foo = dmap[fn]

            if not st and state in "nma":
                dadd(fn)
            elif state == 'n':
                if (size >= 0 and
                    (size != st.st_size
                     or ((mode ^ st.st_mode) & 0100 and self._checkexec))
                    or size == -2
                    or fn in self._copymap):
                    madd(fn)
                elif time != int(st.st_mtime):
                    ladd(fn)
                elif listclean:
                    cadd(fn)
            elif state == 'm':
                madd(fn)
            elif state == 'a':
                aadd(fn)
            elif state == 'r':
                radd(fn)

        return (lookup, modified, added, removed, deleted, unknown, ignored,
                clean)
