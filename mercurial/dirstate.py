# dirstate.py - working directory tracking for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from node import nullid
from i18n import _
import scmutil, util, osutil, parsers, encoding, pathutil
import os, stat, errno
import match as matchmod

propertycache = util.propertycache
filecache = scmutil.filecache
_rangemask = 0x7fffffff

dirstatetuple = parsers.dirstatetuple

class repocache(filecache):
    """filecache for files in .hg/"""
    def join(self, obj, fname):
        return obj._opener.join(fname)

class rootcache(filecache):
    """filecache for files in the repository root"""
    def join(self, obj, fname):
        return obj._join(fname)

class dirstate(object):

    def __init__(self, opener, ui, root, validate):
        '''Create a new dirstate object.

        opener is an open()-like callable that can be used to open the
        dirstate file; root is the root of the directory tracked by
        the dirstate.
        '''
        self._opener = opener
        self._validate = validate
        self._root = root
        # ntpath.join(root, '') of Python 2.7.9 does not add sep if root is
        # UNC path pointing to root share (issue4557)
        self._rootdir = pathutil.normasprefix(root)
        self._dirty = False
        self._dirtypl = False
        self._lastnormaltime = 0
        self._ui = ui
        self._filecache = {}
        self._parentwriters = 0
        self._filename = 'dirstate'

    def beginparentchange(self):
        '''Marks the beginning of a set of changes that involve changing
        the dirstate parents. If there is an exception during this time,
        the dirstate will not be written when the wlock is released. This
        prevents writing an incoherent dirstate where the parent doesn't
        match the contents.
        '''
        self._parentwriters += 1

    def endparentchange(self):
        '''Marks the end of a set of changes that involve changing the
        dirstate parents. Once all parent changes have been marked done,
        the wlock will be free to write the dirstate on release.
        '''
        if self._parentwriters > 0:
            self._parentwriters -= 1

    def pendingparentchange(self):
        '''Returns true if the dirstate is in the middle of a set of changes
        that modify the dirstate parent.
        '''
        return self._parentwriters > 0

    @propertycache
    def _map(self):
        '''Return the dirstate contents as a map from filename to
        (state, mode, size, time).'''
        self._read()
        return self._map

    @propertycache
    def _copymap(self):
        self._read()
        return self._copymap

    @propertycache
    def _filefoldmap(self):
        try:
            makefilefoldmap = parsers.make_file_foldmap
        except AttributeError:
            pass
        else:
            return makefilefoldmap(self._map, util.normcasespec,
                                   util.normcasefallback)

        f = {}
        normcase = util.normcase
        for name, s in self._map.iteritems():
            if s[0] != 'r':
                f[normcase(name)] = name
        f['.'] = '.' # prevents useless util.fspath() invocation
        return f

    @propertycache
    def _dirfoldmap(self):
        f = {}
        normcase = util.normcase
        for name in self._dirs:
            f[normcase(name)] = name
        return f

    @repocache('branch')
    def _branch(self):
        try:
            return self._opener.read("branch").strip() or "default"
        except IOError, inst:
            if inst.errno != errno.ENOENT:
                raise
            return "default"

    @propertycache
    def _pl(self):
        try:
            fp = self._opener(self._filename)
            st = fp.read(40)
            fp.close()
            l = len(st)
            if l == 40:
                return st[:20], st[20:40]
            elif l > 0 and l < 40:
                raise util.Abort(_('working directory state appears damaged!'))
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
        return [nullid, nullid]

    @propertycache
    def _dirs(self):
        return util.dirs(self._map, 'r')

    def dirs(self):
        return self._dirs

    @rootcache('.hgignore')
    def _ignore(self):
        files = []
        if os.path.exists(self._join('.hgignore')):
            files.append(self._join('.hgignore'))
        for name, path in self._ui.configitems("ui"):
            if name == 'ignore' or name.startswith('ignore.'):
                # we need to use os.path.join here rather than self._join
                # because path is arbitrary and user-specified
                files.append(os.path.join(self._rootdir, util.expandpath(path)))

        if not files:
            return util.never

        pats = ['include:%s' % f for f in files]
        return matchmod.match(self._root, '', [], pats, warn=self._ui.warn)

    @propertycache
    def _slash(self):
        return self._ui.configbool('ui', 'slash') and os.sep != '/'

    @propertycache
    def _checklink(self):
        return util.checklink(self._root)

    @propertycache
    def _checkexec(self):
        return util.checkexec(self._root)

    @propertycache
    def _checkcase(self):
        return not util.checkcase(self._join('.hg'))

    def _join(self, f):
        # much faster than os.path.join()
        # it's safe because f is always a relative path
        return self._rootdir + f

    def flagfunc(self, buildfallback):
        if self._checklink and self._checkexec:
            def f(x):
                try:
                    st = os.lstat(self._join(x))
                    if util.statislink(st):
                        return 'l'
                    if util.statisexec(st):
                        return 'x'
                except OSError:
                    pass
                return ''
            return f

        fallback = buildfallback()
        if self._checklink:
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
                if util.isexec(self._join(x)):
                    return 'x'
                return ''
            return f
        else:
            return fallback

    @propertycache
    def _cwd(self):
        return os.getcwd()

    def getcwd(self):
        cwd = self._cwd
        if cwd == self._root:
            return ''
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
            return util.pconvert(path)
        return path

    def __getitem__(self, key):
        '''Return the current state of key (a filename) in the dirstate.

        States are:
          n  normal
          m  needs merging
          r  marked for removal
          a  marked for addition
          ?  not tracked
        '''
        return self._map.get(key, ("?",))[0]

    def __contains__(self, key):
        return key in self._map

    def __iter__(self):
        for x in sorted(self._map):
            yield x

    def iteritems(self):
        return self._map.iteritems()

    def parents(self):
        return [self._validate(p) for p in self._pl]

    def p1(self):
        return self._validate(self._pl[0])

    def p2(self):
        return self._validate(self._pl[1])

    def branch(self):
        return encoding.tolocal(self._branch)

    def setparents(self, p1, p2=nullid):
        """Set dirstate parents to p1 and p2.

        When moving from two parents to one, 'm' merged entries a
        adjusted to normal and previous copy records discarded and
        returned by the call.

        See localrepo.setparents()
        """
        if self._parentwriters == 0:
            raise ValueError("cannot set dirstate parent without "
                             "calling dirstate.beginparentchange")

        self._dirty = self._dirtypl = True
        oldp2 = self._pl[1]
        self._pl = p1, p2
        copies = {}
        if oldp2 != nullid and p2 == nullid:
            for f, s in self._map.iteritems():
                # Discard 'm' markers when moving away from a merge state
                if s[0] == 'm':
                    if f in self._copymap:
                        copies[f] = self._copymap[f]
                    self.normallookup(f)
                # Also fix up otherparent markers
                elif s[0] == 'n' and s[2] == -2:
                    if f in self._copymap:
                        copies[f] = self._copymap[f]
                    self.add(f)
        return copies

    def setbranch(self, branch):
        self._branch = encoding.fromlocal(branch)
        f = self._opener('branch', 'w', atomictemp=True)
        try:
            f.write(self._branch + '\n')
            f.close()

            # make sure filecache has the correct stat info for _branch after
            # replacing the underlying file
            ce = self._filecache['_branch']
            if ce:
                ce.refresh()
        except: # re-raises
            f.discard()
            raise

    def _read(self):
        self._map = {}
        self._copymap = {}
        try:
            fp = self._opener.open(self._filename)
            try:
                st = fp.read()
            finally:
                fp.close()
        except IOError, err:
            if err.errno != errno.ENOENT:
                raise
            return
        if not st:
            return

        if util.safehasattr(parsers, 'dict_new_presized'):
            # Make an estimate of the number of files in the dirstate based on
            # its size. From a linear regression on a set of real-world repos,
            # all over 10,000 files, the size of a dirstate entry is 85
            # bytes. The cost of resizing is significantly higher than the cost
            # of filling in a larger presized dict, so subtract 20% from the
            # size.
            #
            # This heuristic is imperfect in many ways, so in a future dirstate
            # format update it makes sense to just record the number of entries
            # on write.
            self._map = parsers.dict_new_presized(len(st) / 71)

        # Python's garbage collector triggers a GC each time a certain number
        # of container objects (the number being defined by
        # gc.get_threshold()) are allocated. parse_dirstate creates a tuple
        # for each file in the dirstate. The C version then immediately marks
        # them as not to be tracked by the collector. However, this has no
        # effect on when GCs are triggered, only on what objects the GC looks
        # into. This means that O(number of files) GCs are unavoidable.
        # Depending on when in the process's lifetime the dirstate is parsed,
        # this can get very expensive. As a workaround, disable GC while
        # parsing the dirstate.
        #
        # (we cannot decorate the function directly since it is in a C module)
        parse_dirstate = util.nogc(parsers.parse_dirstate)
        p = parse_dirstate(self._map, self._copymap, st)
        if not self._dirtypl:
            self._pl = p

    def invalidate(self):
        for a in ("_map", "_copymap", "_filefoldmap", "_dirfoldmap", "_branch",
                  "_pl", "_dirs", "_ignore"):
            if a in self.__dict__:
                delattr(self, a)
        self._lastnormaltime = 0
        self._dirty = False
        self._parentwriters = 0

    def copy(self, source, dest):
        """Mark dest as a copy of source. Unmark dest if source is None."""
        if source == dest:
            return
        self._dirty = True
        if source is not None:
            self._copymap[dest] = source
        elif dest in self._copymap:
            del self._copymap[dest]

    def copied(self, file):
        return self._copymap.get(file, None)

    def copies(self):
        return self._copymap

    def _droppath(self, f):
        if self[f] not in "?r" and "_dirs" in self.__dict__:
            self._dirs.delpath(f)

    def _addpath(self, f, state, mode, size, mtime):
        oldstate = self[f]
        if state == 'a' or oldstate == 'r':
            scmutil.checkfilename(f)
            if f in self._dirs:
                raise util.Abort(_('directory %r already in dirstate') % f)
            # shadows
            for d in util.finddirs(f):
                if d in self._dirs:
                    break
                if d in self._map and self[d] != 'r':
                    raise util.Abort(
                        _('file %r in dirstate clashes with %r') % (d, f))
        if oldstate in "?r" and "_dirs" in self.__dict__:
            self._dirs.addpath(f)
        self._dirty = True
        self._map[f] = dirstatetuple(state, mode, size, mtime)

    def normal(self, f):
        '''Mark a file normal and clean.'''
        s = os.lstat(self._join(f))
        mtime = int(s.st_mtime)
        self._addpath(f, 'n', s.st_mode,
                      s.st_size & _rangemask, mtime & _rangemask)
        if f in self._copymap:
            del self._copymap[f]
        if mtime > self._lastnormaltime:
            # Remember the most recent modification timeslot for status(),
            # to make sure we won't miss future size-preserving file content
            # modifications that happen within the same timeslot.
            self._lastnormaltime = mtime

    def normallookup(self, f):
        '''Mark a file normal, but possibly dirty.'''
        if self._pl[1] != nullid and f in self._map:
            # if there is a merge going on and the file was either
            # in state 'm' (-1) or coming from other parent (-2) before
            # being removed, restore that state.
            entry = self._map[f]
            if entry[0] == 'r' and entry[2] in (-1, -2):
                source = self._copymap.get(f)
                if entry[2] == -1:
                    self.merge(f)
                elif entry[2] == -2:
                    self.otherparent(f)
                if source:
                    self.copy(source, f)
                return
            if entry[0] == 'm' or entry[0] == 'n' and entry[2] == -2:
                return
        self._addpath(f, 'n', 0, -1, -1)
        if f in self._copymap:
            del self._copymap[f]

    def otherparent(self, f):
        '''Mark as coming from the other parent, always dirty.'''
        if self._pl[1] == nullid:
            raise util.Abort(_("setting %r to other parent "
                               "only allowed in merges") % f)
        if f in self and self[f] == 'n':
            # merge-like
            self._addpath(f, 'm', 0, -2, -1)
        else:
            # add-like
            self._addpath(f, 'n', 0, -2, -1)

        if f in self._copymap:
            del self._copymap[f]

    def add(self, f):
        '''Mark a file added.'''
        self._addpath(f, 'a', 0, -1, -1)
        if f in self._copymap:
            del self._copymap[f]

    def remove(self, f):
        '''Mark a file removed.'''
        self._dirty = True
        self._droppath(f)
        size = 0
        if self._pl[1] != nullid and f in self._map:
            # backup the previous state
            entry = self._map[f]
            if entry[0] == 'm': # merge
                size = -1
            elif entry[0] == 'n' and entry[2] == -2: # other parent
                size = -2
        self._map[f] = dirstatetuple('r', 0, size, 0)
        if size == 0 and f in self._copymap:
            del self._copymap[f]

    def merge(self, f):
        '''Mark a file merged.'''
        if self._pl[1] == nullid:
            return self.normallookup(f)
        return self.otherparent(f)

    def drop(self, f):
        '''Drop a file from the dirstate'''
        if f in self._map:
            self._dirty = True
            self._droppath(f)
            del self._map[f]

    def _discoverpath(self, path, normed, ignoremissing, exists, storemap):
        if exists is None:
            exists = os.path.lexists(os.path.join(self._root, path))
        if not exists:
            # Maybe a path component exists
            if not ignoremissing and '/' in path:
                d, f = path.rsplit('/', 1)
                d = self._normalize(d, False, ignoremissing, None)
                folded = d + "/" + f
            else:
                # No path components, preserve original case
                folded = path
        else:
            # recursively normalize leading directory components
            # against dirstate
            if '/' in normed:
                d, f = normed.rsplit('/', 1)
                d = self._normalize(d, False, ignoremissing, True)
                r = self._root + "/" + d
                folded = d + "/" + util.fspath(f, r)
            else:
                folded = util.fspath(normed, self._root)
            storemap[normed] = folded

        return folded

    def _normalizefile(self, path, isknown, ignoremissing=False, exists=None):
        normed = util.normcase(path)
        folded = self._filefoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                folded = self._discoverpath(path, normed, ignoremissing, exists,
                                            self._filefoldmap)
        return folded

    def _normalize(self, path, isknown, ignoremissing=False, exists=None):
        normed = util.normcase(path)
        folded = self._filefoldmap.get(normed, None)
        if folded is None:
            folded = self._dirfoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                # store discovered result in dirfoldmap so that future
                # normalizefile calls don't start matching directories
                folded = self._discoverpath(path, normed, ignoremissing, exists,
                                            self._dirfoldmap)
        return folded

    def normalize(self, path, isknown=False, ignoremissing=False):
        '''
        normalize the case of a pathname when on a casefolding filesystem

        isknown specifies whether the filename came from walking the
        disk, to avoid extra filesystem access.

        If ignoremissing is True, missing path are returned
        unchanged. Otherwise, we try harder to normalize possibly
        existing path components.

        The normalized case is determined based on the following precedence:

        - version of name already stored in the dirstate
        - version of name stored on disk
        - version provided via command arguments
        '''

        if self._checkcase:
            return self._normalize(path, isknown, ignoremissing)
        return path

    def clear(self):
        self._map = {}
        if "_dirs" in self.__dict__:
            delattr(self, "_dirs")
        self._copymap = {}
        self._pl = [nullid, nullid]
        self._lastnormaltime = 0
        self._dirty = True

    def rebuild(self, parent, allfiles, changedfiles=None):
        if changedfiles is None:
            changedfiles = allfiles
        oldmap = self._map
        self.clear()
        for f in allfiles:
            if f not in changedfiles:
                self._map[f] = oldmap[f]
            else:
                if 'x' in allfiles.flags(f):
                    self._map[f] = dirstatetuple('n', 0777, -1, 0)
                else:
                    self._map[f] = dirstatetuple('n', 0666, -1, 0)
        self._pl = (parent, nullid)
        self._dirty = True

    def write(self):
        if not self._dirty:
            return

        # enough 'delaywrite' prevents 'pack_dirstate' from dropping
        # timestamp of each entries in dirstate, because of 'now > mtime'
        delaywrite = self._ui.configint('debug', 'dirstate.delaywrite', 0)
        if delaywrite > 0:
            import time # to avoid useless import
            time.sleep(delaywrite)

        st = self._opener(self._filename, "w", atomictemp=True)
        # use the modification time of the newly created temporary file as the
        # filesystem's notion of 'now'
        now = util.fstat(st).st_mtime
        st.write(parsers.pack_dirstate(self._map, self._copymap, self._pl, now))
        st.close()
        self._lastnormaltime = 0
        self._dirty = self._dirtypl = False

    def _dirignore(self, f):
        if f == '.':
            return False
        if self._ignore(f):
            return True
        for p in util.finddirs(f):
            if self._ignore(p):
                return True
        return False

    def _walkexplicit(self, match, subrepos):
        '''Get stat data about the files explicitly specified by match.

        Return a triple (results, dirsfound, dirsnotfound).
        - results is a mapping from filename to stat result. It also contains
          listings mapping subrepos and .hg to None.
        - dirsfound is a list of files found to be directories.
        - dirsnotfound is a list of files that the dirstate thinks are
          directories and that were not found.'''

        def badtype(mode):
            kind = _('unknown')
            if stat.S_ISCHR(mode):
                kind = _('character device')
            elif stat.S_ISBLK(mode):
                kind = _('block device')
            elif stat.S_ISFIFO(mode):
                kind = _('fifo')
            elif stat.S_ISSOCK(mode):
                kind = _('socket')
            elif stat.S_ISDIR(mode):
                kind = _('directory')
            return _('unsupported file type (type is %s)') % kind

        matchedir = match.explicitdir
        badfn = match.bad
        dmap = self._map
        lstat = os.lstat
        getkind = stat.S_IFMT
        dirkind = stat.S_IFDIR
        regkind = stat.S_IFREG
        lnkkind = stat.S_IFLNK
        join = self._join
        dirsfound = []
        foundadd = dirsfound.append
        dirsnotfound = []
        notfoundadd = dirsnotfound.append

        if not match.isexact() and self._checkcase:
            normalize = self._normalize
        else:
            normalize = None

        files = sorted(match.files())
        subrepos.sort()
        i, j = 0, 0
        while i < len(files) and j < len(subrepos):
            subpath = subrepos[j] + "/"
            if files[i] < subpath:
                i += 1
                continue
            while i < len(files) and files[i].startswith(subpath):
                del files[i]
            j += 1

        if not files or '.' in files:
            files = ['.']
        results = dict.fromkeys(subrepos)
        results['.hg'] = None

        alldirs = None
        for ff in files:
            # constructing the foldmap is expensive, so don't do it for the
            # common case where files is ['.']
            if normalize and ff != '.':
                nf = normalize(ff, False, True)
            else:
                nf = ff
            if nf in results:
                continue

            try:
                st = lstat(join(nf))
                kind = getkind(st.st_mode)
                if kind == dirkind:
                    if nf in dmap:
                        # file replaced by dir on disk but still in dirstate
                        results[nf] = None
                    if matchedir:
                        matchedir(nf)
                    foundadd((nf, ff))
                elif kind == regkind or kind == lnkkind:
                    results[nf] = st
                else:
                    badfn(ff, badtype(kind))
                    if nf in dmap:
                        results[nf] = None
            except OSError, inst: # nf not found on disk - it is dirstate only
                if nf in dmap: # does it exactly match a missing file?
                    results[nf] = None
                else: # does it match a missing directory?
                    if alldirs is None:
                        alldirs = util.dirs(dmap)
                    if nf in alldirs:
                        if matchedir:
                            matchedir(nf)
                        notfoundadd(nf)
                    else:
                        badfn(ff, inst.strerror)

        return results, dirsfound, dirsnotfound

    def walk(self, match, subrepos, unknown, ignored, full=True):
        '''
        Walk recursively through the directory tree, finding all files
        matched by match.

        If full is False, maybe skip some known-clean files.

        Return a dict mapping filename to stat-like object (either
        mercurial.osutil.stat instance or return value of os.stat()).

        '''
        # full is a flag that extensions that hook into walk can use -- this
        # implementation doesn't use it at all. This satisfies the contract
        # because we only guarantee a "maybe".

        if ignored:
            ignore = util.never
            dirignore = util.never
        elif unknown:
            ignore = self._ignore
            dirignore = self._dirignore
        else:
            # if not unknown and not ignored, drop dir recursion and step 2
            ignore = util.always
            dirignore = util.always

        matchfn = match.matchfn
        matchalways = match.always()
        matchtdir = match.traversedir
        dmap = self._map
        listdir = osutil.listdir
        lstat = os.lstat
        dirkind = stat.S_IFDIR
        regkind = stat.S_IFREG
        lnkkind = stat.S_IFLNK
        join = self._join

        exact = skipstep3 = False
        if match.isexact(): # match.exact
            exact = True
            dirignore = util.always # skip step 2
        elif match.prefix(): # match.match, no patterns
            skipstep3 = True

        if not exact and self._checkcase:
            normalize = self._normalize
            normalizefile = self._normalizefile
            skipstep3 = False
        else:
            normalize = self._normalize
            normalizefile = None

        # step 1: find all explicit files
        results, work, dirsnotfound = self._walkexplicit(match, subrepos)

        skipstep3 = skipstep3 and not (work or dirsnotfound)
        work = [d for d in work if not dirignore(d[0])]

        # step 2: visit subdirectories
        def traverse(work, alreadynormed):
            wadd = work.append
            while work:
                nd = work.pop()
                skip = None
                if nd == '.':
                    nd = ''
                else:
                    skip = '.hg'
                try:
                    entries = listdir(join(nd), stat=True, skip=skip)
                except OSError, inst:
                    if inst.errno in (errno.EACCES, errno.ENOENT):
                        match.bad(self.pathto(nd), inst.strerror)
                        continue
                    raise
                for f, kind, st in entries:
                    if normalizefile:
                        # even though f might be a directory, we're only
                        # interested in comparing it to files currently in the
                        # dmap -- therefore normalizefile is enough
                        nf = normalizefile(nd and (nd + "/" + f) or f, True,
                                           True)
                    else:
                        nf = nd and (nd + "/" + f) or f
                    if nf not in results:
                        if kind == dirkind:
                            if not ignore(nf):
                                if matchtdir:
                                    matchtdir(nf)
                                wadd(nf)
                            if nf in dmap and (matchalways or matchfn(nf)):
                                results[nf] = None
                        elif kind == regkind or kind == lnkkind:
                            if nf in dmap:
                                if matchalways or matchfn(nf):
                                    results[nf] = st
                            elif ((matchalways or matchfn(nf))
                                  and not ignore(nf)):
                                # unknown file -- normalize if necessary
                                if not alreadynormed:
                                    nf = normalize(nf, False, True)
                                results[nf] = st
                        elif nf in dmap and (matchalways or matchfn(nf)):
                            results[nf] = None

        for nd, d in work:
            # alreadynormed means that processwork doesn't have to do any
            # expensive directory normalization
            alreadynormed = not normalize or nd == d
            traverse([d], alreadynormed)

        for s in subrepos:
            del results[s]
        del results['.hg']

        # step 3: visit remaining files from dmap
        if not skipstep3 and not exact:
            # If a dmap file is not in results yet, it was either
            # a) not matching matchfn b) ignored, c) missing, or d) under a
            # symlink directory.
            if not results and matchalways:
                visit = dmap.keys()
            else:
                visit = [f for f in dmap if f not in results and matchfn(f)]
            visit.sort()

            if unknown:
                # unknown == True means we walked all dirs under the roots
                # that wasn't ignored, and everything that matched was stat'ed
                # and is already in results.
                # The rest must thus be ignored or under a symlink.
                audit_path = pathutil.pathauditor(self._root)

                for nf in iter(visit):
                    # If a stat for the same file was already added with a
                    # different case, don't add one for this, since that would
                    # make it appear as if the file exists under both names
                    # on disk.
                    if (normalizefile and
                        normalizefile(nf, True, True) in results):
                        results[nf] = None
                    # Report ignored items in the dmap as long as they are not
                    # under a symlink directory.
                    elif audit_path.check(nf):
                        try:
                            results[nf] = lstat(join(nf))
                            # file was just ignored, no links, and exists
                        except OSError:
                            # file doesn't exist
                            results[nf] = None
                    else:
                        # It's either missing or under a symlink directory
                        # which we in this case report as missing
                        results[nf] = None
            else:
                # We may not have walked the full directory tree above,
                # so stat and check everything we missed.
                nf = iter(visit).next
                for st in util.statfiles([join(i) for i in visit]):
                    results[nf()] = st
        return results

    def status(self, match, subrepos, ignored, clean, unknown):
        '''Determine the status of the working copy relative to the
        dirstate and return a pair of (unsure, status), where status is of type
        scmutil.status and:

          unsure:
            files that might have been modified since the dirstate was
            written, but need to be read to be sure (size is the same
            but mtime differs)
          status.modified:
            files that have definitely been modified since the dirstate
            was written (different size or mode)
          status.clean:
            files that have definitely not been modified since the
            dirstate was written
        '''
        listignored, listclean, listunknown = ignored, clean, unknown
        lookup, modified, added, unknown, ignored = [], [], [], [], []
        removed, deleted, clean = [], [], []

        dmap = self._map
        ladd = lookup.append            # aka "unsure"
        madd = modified.append
        aadd = added.append
        uadd = unknown.append
        iadd = ignored.append
        radd = removed.append
        dadd = deleted.append
        cadd = clean.append
        mexact = match.exact
        dirignore = self._dirignore
        checkexec = self._checkexec
        copymap = self._copymap
        lastnormaltime = self._lastnormaltime

        # We need to do full walks when either
        # - we're listing all clean files, or
        # - match.traversedir does something, because match.traversedir should
        #   be called for every dir in the working dir
        full = listclean or match.traversedir is not None
        for fn, st in self.walk(match, subrepos, listunknown, listignored,
                                full=full).iteritems():
            if fn not in dmap:
                if (listignored or mexact(fn)) and dirignore(fn):
                    if listignored:
                        iadd(fn)
                else:
                    uadd(fn)
                continue

            # This is equivalent to 'state, mode, size, time = dmap[fn]' but not
            # written like that for performance reasons. dmap[fn] is not a
            # Python tuple in compiled builds. The CPython UNPACK_SEQUENCE
            # opcode has fast paths when the value to be unpacked is a tuple or
            # a list, but falls back to creating a full-fledged iterator in
            # general. That is much slower than simply accessing and storing the
            # tuple members one by one.
            t = dmap[fn]
            state = t[0]
            mode = t[1]
            size = t[2]
            time = t[3]

            if not st and state in "nma":
                dadd(fn)
            elif state == 'n':
                mtime = int(st.st_mtime)
                if (size >= 0 and
                    ((size != st.st_size and size != st.st_size & _rangemask)
                     or ((mode ^ st.st_mode) & 0100 and checkexec))
                    or size == -2 # other parent
                    or fn in copymap):
                    madd(fn)
                elif time != mtime and time != mtime & _rangemask:
                    ladd(fn)
                elif mtime == lastnormaltime:
                    # fn may have just been marked as normal and it may have
                    # changed in the same second without changing its size.
                    # This can happen if we quickly do multiple commits.
                    # Force lookup, so we don't miss such a racy file change.
                    ladd(fn)
                elif listclean:
                    cadd(fn)
            elif state == 'm':
                madd(fn)
            elif state == 'a':
                aadd(fn)
            elif state == 'r':
                radd(fn)

        return (lookup, scmutil.status(modified, added, removed, deleted,
                                       unknown, ignored, clean))

    def matches(self, match):
        '''
        return files in the dirstate (in whatever state) filtered by match
        '''
        dmap = self._map
        if match.always():
            return dmap.keys()
        files = match.files()
        if match.isexact():
            # fast path -- filter the other way around, since typically files is
            # much smaller than dmap
            return [f for f in files if f in dmap]
        if match.prefix() and all(fn in dmap for fn in files):
            # fast path -- all the values are known to be files, so just return
            # that
            return list(files)
        return [f for f in dmap if match(f)]
