# dirstate.py - working directory tracking for mercurial
#
# Copyright 2005-2007 Matt Mackall <mpm@selenic.com>
#
# This software may be used and distributed according to the terms of the
# GNU General Public License version 2 or any later version.

from __future__ import absolute_import

import collections
import contextlib
import errno
import os
import stat

from .i18n import _
from .node import nullid
from . import (
    encoding,
    error,
    match as matchmod,
    pathutil,
    policy,
    pycompat,
    scmutil,
    txnutil,
    util,
)

parsers = policy.importmod(r'parsers')

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

def _getfsnow(vfs):
    '''Get "now" timestamp on filesystem'''
    tmpfd, tmpname = vfs.mkstemp()
    try:
        return os.fstat(tmpfd).st_mtime
    finally:
        os.close(tmpfd)
        vfs.unlink(tmpname)

class dirstate(object):

    def __init__(self, opener, ui, root, validate, sparsematchfn):
        '''Create a new dirstate object.

        opener is an open()-like callable that can be used to open the
        dirstate file; root is the root of the directory tracked by
        the dirstate.
        '''
        self._opener = opener
        self._validate = validate
        self._root = root
        self._sparsematchfn = sparsematchfn
        # ntpath.join(root, '') of Python 2.7.9 does not add sep if root is
        # UNC path pointing to root share (issue4557)
        self._rootdir = pathutil.normasprefix(root)
        self._dirty = False
        self._lastnormaltime = 0
        self._ui = ui
        self._filecache = {}
        self._parentwriters = 0
        self._filename = 'dirstate'
        self._pendingfilename = '%s.pending' % self._filename
        self._plchangecallbacks = {}
        self._origpl = None
        self._updatedfiles = set()
        self._mapcls = dirstatemap

    @contextlib.contextmanager
    def parentchange(self):
        '''Context manager for handling dirstate parents.

        If an exception occurs in the scope of the context manager,
        the incoherent dirstate won't be written when wlock is
        released.
        '''
        self._parentwriters += 1
        yield
        # Typically we want the "undo" step of a context manager in a
        # finally block so it happens even when an exception
        # occurs. In this case, however, we only want to decrement
        # parentwriters if the code in the with statement exits
        # normally, so we don't have a try/finally here on purpose.
        self._parentwriters -= 1

    def beginparentchange(self):
        '''Marks the beginning of a set of changes that involve changing
        the dirstate parents. If there is an exception during this time,
        the dirstate will not be written when the wlock is released. This
        prevents writing an incoherent dirstate where the parent doesn't
        match the contents.
        '''
        self._ui.deprecwarn('beginparentchange is obsoleted by the '
                            'parentchange context manager.', '4.3')
        self._parentwriters += 1

    def endparentchange(self):
        '''Marks the end of a set of changes that involve changing the
        dirstate parents. Once all parent changes have been marked done,
        the wlock will be free to write the dirstate on release.
        '''
        self._ui.deprecwarn('endparentchange is obsoleted by the '
                            'parentchange context manager.', '4.3')
        if self._parentwriters > 0:
            self._parentwriters -= 1

    def pendingparentchange(self):
        '''Returns true if the dirstate is in the middle of a set of changes
        that modify the dirstate parent.
        '''
        return self._parentwriters > 0

    @propertycache
    def _map(self):
        """Return the dirstate contents (see documentation for dirstatemap)."""
        self._map = self._mapcls(self._ui, self._opener, self._root)
        return self._map

    @property
    def _sparsematcher(self):
        """The matcher for the sparse checkout.

        The working directory may not include every file from a manifest. The
        matcher obtained by this property will match a path if it is to be
        included in the working directory.
        """
        # TODO there is potential to cache this property. For now, the matcher
        # is resolved on every access. (But the called function does use a
        # cache to keep the lookup fast.)
        return self._sparsematchfn()

    @repocache('branch')
    def _branch(self):
        try:
            return self._opener.read("branch").strip() or "default"
        except IOError as inst:
            if inst.errno != errno.ENOENT:
                raise
            return "default"

    @property
    def _pl(self):
        return self._map.parents()

    def hasdir(self, d):
        return self._map.hastrackeddir(d)

    @rootcache('.hgignore')
    def _ignore(self):
        files = self._ignorefiles()
        if not files:
            return matchmod.never(self._root, '')

        pats = ['include:%s' % f for f in files]
        return matchmod.match(self._root, '', [], pats, warn=self._ui.warn)

    @propertycache
    def _slash(self):
        return self._ui.configbool('ui', 'slash') and pycompat.ossep != '/'

    @propertycache
    def _checklink(self):
        return util.checklink(self._root)

    @propertycache
    def _checkexec(self):
        return util.checkexec(self._root)

    @propertycache
    def _checkcase(self):
        return not util.fscasesensitive(self._join('.hg'))

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
        # internal config: ui.forcecwd
        forcecwd = self._ui.config('ui', 'forcecwd')
        if forcecwd:
            return forcecwd
        return pycompat.getcwd()

    def getcwd(self):
        '''Return the path from which a canonical path is calculated.

        This path should be used to resolve file patterns or to convert
        canonical paths back to file paths for display. It shouldn't be
        used to get real file paths. Use vfs functions instead.
        '''
        cwd = self._cwd
        if cwd == self._root:
            return ''
        # self._root ends with a path separator if self._root is '/' or 'C:\'
        rootsep = self._root
        if not util.endswithsep(rootsep):
            rootsep += pycompat.ossep
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
        return iter(sorted(self._map))

    def items(self):
        return self._map.iteritems()

    iteritems = items

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

        self._dirty = True
        oldp2 = self._pl[1]
        if self._origpl is None:
            self._origpl = self._pl
        self._map.setparents(p1, p2)
        copies = {}
        if oldp2 != nullid and p2 == nullid:
            candidatefiles = self._map.nonnormalset.union(
                                self._map.otherparentset)
            for f in candidatefiles:
                s = self._map.get(f)
                if s is None:
                    continue

                # Discard 'm' markers when moving away from a merge state
                if s[0] == 'm':
                    source = self._map.copymap.get(f)
                    if source:
                        copies[f] = source
                    self.normallookup(f)
                # Also fix up otherparent markers
                elif s[0] == 'n' and s[2] == -2:
                    source = self._map.copymap.get(f)
                    if source:
                        copies[f] = source
                    self.add(f)
        return copies

    def setbranch(self, branch):
        self._branch = encoding.fromlocal(branch)
        f = self._opener('branch', 'w', atomictemp=True, checkambig=True)
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

    def invalidate(self):
        '''Causes the next access to reread the dirstate.

        This is different from localrepo.invalidatedirstate() because it always
        rereads the dirstate. Use localrepo.invalidatedirstate() if you want to
        check whether the dirstate has changed before rereading it.'''

        for a in ("_map", "_branch", "_ignore"):
            if a in self.__dict__:
                delattr(self, a)
        self._lastnormaltime = 0
        self._dirty = False
        self._updatedfiles.clear()
        self._parentwriters = 0
        self._origpl = None

    def copy(self, source, dest):
        """Mark dest as a copy of source. Unmark dest if source is None."""
        if source == dest:
            return
        self._dirty = True
        if source is not None:
            self._map.copymap[dest] = source
            self._updatedfiles.add(source)
            self._updatedfiles.add(dest)
        elif self._map.copymap.pop(dest, None):
            self._updatedfiles.add(dest)

    def copied(self, file):
        return self._map.copymap.get(file, None)

    def copies(self):
        return self._map.copymap

    def _addpath(self, f, state, mode, size, mtime):
        oldstate = self[f]
        if state == 'a' or oldstate == 'r':
            scmutil.checkfilename(f)
            if self._map.hastrackeddir(f):
                raise error.Abort(_('directory %r already in dirstate') % f)
            # shadows
            for d in util.finddirs(f):
                if self._map.hastrackeddir(d):
                    break
                entry = self._map.get(d)
                if entry is not None and entry[0] != 'r':
                    raise error.Abort(
                        _('file %r in dirstate clashes with %r') % (d, f))
        self._dirty = True
        self._updatedfiles.add(f)
        self._map.addfile(f, oldstate, state, mode, size, mtime)

    def normal(self, f):
        '''Mark a file normal and clean.'''
        s = os.lstat(self._join(f))
        mtime = s.st_mtime
        self._addpath(f, 'n', s.st_mode,
                      s.st_size & _rangemask, mtime & _rangemask)
        self._map.copymap.pop(f, None)
        if f in self._map.nonnormalset:
            self._map.nonnormalset.remove(f)
        if mtime > self._lastnormaltime:
            # Remember the most recent modification timeslot for status(),
            # to make sure we won't miss future size-preserving file content
            # modifications that happen within the same timeslot.
            self._lastnormaltime = mtime

    def normallookup(self, f):
        '''Mark a file normal, but possibly dirty.'''
        if self._pl[1] != nullid:
            # if there is a merge going on and the file was either
            # in state 'm' (-1) or coming from other parent (-2) before
            # being removed, restore that state.
            entry = self._map.get(f)
            if entry is not None:
                if entry[0] == 'r' and entry[2] in (-1, -2):
                    source = self._map.copymap.get(f)
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
        self._map.copymap.pop(f, None)

    def otherparent(self, f):
        '''Mark as coming from the other parent, always dirty.'''
        if self._pl[1] == nullid:
            raise error.Abort(_("setting %r to other parent "
                               "only allowed in merges") % f)
        if f in self and self[f] == 'n':
            # merge-like
            self._addpath(f, 'm', 0, -2, -1)
        else:
            # add-like
            self._addpath(f, 'n', 0, -2, -1)
        self._map.copymap.pop(f, None)

    def add(self, f):
        '''Mark a file added.'''
        self._addpath(f, 'a', 0, -1, -1)
        self._map.copymap.pop(f, None)

    def remove(self, f):
        '''Mark a file removed.'''
        self._dirty = True
        oldstate = self[f]
        size = 0
        if self._pl[1] != nullid:
            entry = self._map.get(f)
            if entry is not None:
                # backup the previous state
                if entry[0] == 'm': # merge
                    size = -1
                elif entry[0] == 'n' and entry[2] == -2: # other parent
                    size = -2
                    self._map.otherparentset.add(f)
        self._updatedfiles.add(f)
        self._map.removefile(f, oldstate, size)
        if size == 0:
            self._map.copymap.pop(f, None)

    def merge(self, f):
        '''Mark a file merged.'''
        if self._pl[1] == nullid:
            return self.normallookup(f)
        return self.otherparent(f)

    def drop(self, f):
        '''Drop a file from the dirstate'''
        oldstate = self[f]
        if self._map.dropfile(f, oldstate):
            self._dirty = True
            self._updatedfiles.add(f)
            self._map.copymap.pop(f, None)

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
        folded = self._map.filefoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                folded = self._discoverpath(path, normed, ignoremissing, exists,
                                            self._map.filefoldmap)
        return folded

    def _normalize(self, path, isknown, ignoremissing=False, exists=None):
        normed = util.normcase(path)
        folded = self._map.filefoldmap.get(normed, None)
        if folded is None:
            folded = self._map.dirfoldmap.get(normed, None)
        if folded is None:
            if isknown:
                folded = path
            else:
                # store discovered result in dirfoldmap so that future
                # normalizefile calls don't start matching directories
                folded = self._discoverpath(path, normed, ignoremissing, exists,
                                            self._map.dirfoldmap)
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
        self._map.clear()
        self._lastnormaltime = 0
        self._updatedfiles.clear()
        self._dirty = True

    def rebuild(self, parent, allfiles, changedfiles=None):
        if changedfiles is None:
            # Rebuild entire dirstate
            changedfiles = allfiles
            lastnormaltime = self._lastnormaltime
            self.clear()
            self._lastnormaltime = lastnormaltime

        if self._origpl is None:
            self._origpl = self._pl
        self._map.setparents(parent, nullid)
        for f in changedfiles:
            if f in allfiles:
                self.normallookup(f)
            else:
                self.drop(f)

        self._dirty = True

    def identity(self):
        '''Return identity of dirstate itself to detect changing in storage

        If identity of previous dirstate is equal to this, writing
        changes based on the former dirstate out can keep consistency.
        '''
        return self._map.identity

    def write(self, tr):
        if not self._dirty:
            return

        filename = self._filename
        if tr:
            # 'dirstate.write()' is not only for writing in-memory
            # changes out, but also for dropping ambiguous timestamp.
            # delayed writing re-raise "ambiguous timestamp issue".
            # See also the wiki page below for detail:
            # https://www.mercurial-scm.org/wiki/DirstateTransactionPlan

            # emulate dropping timestamp in 'parsers.pack_dirstate'
            now = _getfsnow(self._opener)
            self._map.clearambiguoustimes(self._updatedfiles, now)

            # emulate that all 'dirstate.normal' results are written out
            self._lastnormaltime = 0
            self._updatedfiles.clear()

            # delay writing in-memory changes out
            tr.addfilegenerator('dirstate', (self._filename,),
                                self._writedirstate, location='plain')
            return

        st = self._opener(filename, "w", atomictemp=True, checkambig=True)
        self._writedirstate(st)

    def addparentchangecallback(self, category, callback):
        """add a callback to be called when the wd parents are changed

        Callback will be called with the following arguments:
            dirstate, (oldp1, oldp2), (newp1, newp2)

        Category is a unique identifier to allow overwriting an old callback
        with a newer callback.
        """
        self._plchangecallbacks[category] = callback

    def _writedirstate(self, st):
        # notify callbacks about parents change
        if self._origpl is not None and self._origpl != self._pl:
            for c, callback in sorted(self._plchangecallbacks.iteritems()):
                callback(self, self._origpl, self._pl)
            self._origpl = None
        # use the modification time of the newly created temporary file as the
        # filesystem's notion of 'now'
        now = util.fstat(st).st_mtime & _rangemask

        # enough 'delaywrite' prevents 'pack_dirstate' from dropping
        # timestamp of each entries in dirstate, because of 'now > mtime'
        delaywrite = self._ui.configint('debug', 'dirstate.delaywrite')
        if delaywrite > 0:
            # do we have any files to delay for?
            for f, e in self._map.iteritems():
                if e[0] == 'n' and e[3] == now:
                    import time # to avoid useless import
                    # rather than sleep n seconds, sleep until the next
                    # multiple of n seconds
                    clock = time.time()
                    start = int(clock) - (int(clock) % delaywrite)
                    end = start + delaywrite
                    time.sleep(end - clock)
                    now = end # trust our estimate that the end is near now
                    break

        self._map.write(st, now)
        self._lastnormaltime = 0
        self._dirty = False

    def _dirignore(self, f):
        if f == '.':
            return False
        if self._ignore(f):
            return True
        for p in util.finddirs(f):
            if self._ignore(p):
                return True
        return False

    def _ignorefiles(self):
        files = []
        if os.path.exists(self._join('.hgignore')):
            files.append(self._join('.hgignore'))
        for name, path in self._ui.configitems("ui"):
            if name == 'ignore' or name.startswith('ignore.'):
                # we need to use os.path.join here rather than self._join
                # because path is arbitrary and user-specified
                files.append(os.path.join(self._rootdir, util.expandpath(path)))
        return files

    def _ignorefileandline(self, f):
        files = collections.deque(self._ignorefiles())
        visited = set()
        while files:
            i = files.popleft()
            patterns = matchmod.readpatternfile(i, self._ui.warn,
                                                sourceinfo=True)
            for pattern, lineno, line in patterns:
                kind, p = matchmod._patsplit(pattern, 'glob')
                if kind == "subinclude":
                    if p not in visited:
                        files.append(p)
                    continue
                m = matchmod.match(self._root, '', [], [pattern],
                                   warn=self._ui.warn)
                if m(f):
                    return (i, lineno, line)
            visited.add(i)
        return (None, -1, "")

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
            except OSError as inst: # nf not found on disk - it is dirstate only
                if nf in dmap: # does it exactly match a missing file?
                    results[nf] = None
                else: # does it match a missing directory?
                    if self._map.hasdir(nf):
                        if matchedir:
                            matchedir(nf)
                        notfoundadd(nf)
                    else:
                        badfn(ff, encoding.strtolocal(inst.strerror))

        # Case insensitive filesystems cannot rely on lstat() failing to detect
        # a case-only rename.  Prune the stat object for any file that does not
        # match the case in the filesystem, if there are multiple files that
        # normalize to the same path.
        if match.isexact() and self._checkcase:
            normed = {}

            for f, st in results.iteritems():
                if st is None:
                    continue

                nc = util.normcase(f)
                paths = normed.get(nc)

                if paths is None:
                    paths = set()
                    normed[nc] = paths

                paths.add(f)

            for norm, paths in normed.iteritems():
                if len(paths) > 1:
                    for path in paths:
                        folded = self._discoverpath(path, norm, True, None,
                                                    self._map.dirfoldmap)
                        if path != folded:
                            results[path] = None

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
        listdir = util.listdir
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
                if not match.visitdir(nd):
                    continue
                skip = None
                if nd == '.':
                    nd = ''
                else:
                    skip = '.hg'
                try:
                    entries = listdir(join(nd), stat=True, skip=skip)
                except OSError as inst:
                    if inst.errno in (errno.EACCES, errno.ENOENT):
                        match.bad(self.pathto(nd),
                                  encoding.strtolocal(inst.strerror))
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
                visit = [f for f in dmap]
            else:
                visit = [f for f in dmap if f not in results and matchfn(f)]
            visit.sort()

            if unknown:
                # unknown == True means we walked all dirs under the roots
                # that wasn't ignored, and everything that matched was stat'ed
                # and is already in results.
                # The rest must thus be ignored or under a symlink.
                audit_path = pathutil.pathauditor(self._root, cached=True)

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
                iv = iter(visit)
                for st in util.statfiles([join(i) for i in visit]):
                    results[next(iv)] = st
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
        dmap.preload()
        dcontains = dmap.__contains__
        dget = dmap.__getitem__
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
        copymap = self._map.copymap
        lastnormaltime = self._lastnormaltime

        # We need to do full walks when either
        # - we're listing all clean files, or
        # - match.traversedir does something, because match.traversedir should
        #   be called for every dir in the working dir
        full = listclean or match.traversedir is not None
        for fn, st in self.walk(match, subrepos, listunknown, listignored,
                                full=full).iteritems():
            if not dcontains(fn):
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
            t = dget(fn)
            state = t[0]
            mode = t[1]
            size = t[2]
            time = t[3]

            if not st and state in "nma":
                dadd(fn)
            elif state == 'n':
                if (size >= 0 and
                    ((size != st.st_size and size != st.st_size & _rangemask)
                     or ((mode ^ st.st_mode) & 0o100 and checkexec))
                    or size == -2 # other parent
                    or fn in copymap):
                    madd(fn)
                elif time != st.st_mtime and time != st.st_mtime & _rangemask:
                    ladd(fn)
                elif st.st_mtime == lastnormaltime:
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

    def _actualfilename(self, tr):
        if tr:
            return self._pendingfilename
        else:
            return self._filename

    def savebackup(self, tr, backupname):
        '''Save current dirstate into backup file'''
        filename = self._actualfilename(tr)
        assert backupname != filename

        # use '_writedirstate' instead of 'write' to write changes certainly,
        # because the latter omits writing out if transaction is running.
        # output file will be used to create backup of dirstate at this point.
        if self._dirty or not self._opener.exists(filename):
            self._writedirstate(self._opener(filename, "w", atomictemp=True,
                                             checkambig=True))

        if tr:
            # ensure that subsequent tr.writepending returns True for
            # changes written out above, even if dirstate is never
            # changed after this
            tr.addfilegenerator('dirstate', (self._filename,),
                                self._writedirstate, location='plain')

            # ensure that pending file written above is unlinked at
            # failure, even if tr.writepending isn't invoked until the
            # end of this transaction
            tr.registertmp(filename, location='plain')

        self._opener.tryunlink(backupname)
        # hardlink backup is okay because _writedirstate is always called
        # with an "atomictemp=True" file.
        util.copyfile(self._opener.join(filename),
                      self._opener.join(backupname), hardlink=True)

    def restorebackup(self, tr, backupname):
        '''Restore dirstate by backup file'''
        # this "invalidate()" prevents "wlock.release()" from writing
        # changes of dirstate out after restoring from backup file
        self.invalidate()
        filename = self._actualfilename(tr)
        o = self._opener
        if util.samefile(o.join(backupname), o.join(filename)):
            o.unlink(backupname)
        else:
            o.rename(backupname, filename, checkambig=True)

    def clearbackup(self, tr, backupname):
        '''Clear backup file'''
        self._opener.unlink(backupname)

class dirstatemap(object):
    """Map encapsulating the dirstate's contents.

    The dirstate contains the following state:

    - `identity` is the identity of the dirstate file, which can be used to
      detect when changes have occurred to the dirstate file.

    - `parents` is a pair containing the parents of the working copy. The
      parents are updated by calling `setparents`.

    - the state map maps filenames to tuples of (state, mode, size, mtime),
      where state is a single character representing 'normal', 'added',
      'removed', or 'merged'. It is read by treating the dirstate as a
      dict.  File state is updated by calling the `addfile`, `removefile` and
      `dropfile` methods.

    - `copymap` maps destination filenames to their source filename.

    The dirstate also provides the following views onto the state:

    - `nonnormalset` is a set of the filenames that have state other
      than 'normal', or are normal but have an mtime of -1 ('normallookup').

    - `otherparentset` is a set of the filenames that are marked as coming
      from the second parent when the dirstate is currently being merged.

    - `filefoldmap` is a dict mapping normalized filenames to the denormalized
      form that they appear as in the dirstate.

    - `dirfoldmap` is a dict mapping normalized directory names to the
      denormalized form that they appear as in the dirstate.
    """

    def __init__(self, ui, opener, root):
        self._ui = ui
        self._opener = opener
        self._root = root
        self._filename = 'dirstate'

        self._parents = None
        self._dirtyparents = False

        # for consistent view between _pl() and _read() invocations
        self._pendingmode = None

    @propertycache
    def _map(self):
        self._map = {}
        self.read()
        return self._map

    @propertycache
    def copymap(self):
        self.copymap = {}
        self._map
        return self.copymap

    def clear(self):
        self._map.clear()
        self.copymap.clear()
        self.setparents(nullid, nullid)
        util.clearcachedproperty(self, "_dirs")
        util.clearcachedproperty(self, "_alldirs")
        util.clearcachedproperty(self, "filefoldmap")
        util.clearcachedproperty(self, "dirfoldmap")
        util.clearcachedproperty(self, "nonnormalset")
        util.clearcachedproperty(self, "otherparentset")

    def iteritems(self):
        return self._map.iteritems()

    def __len__(self):
        return len(self._map)

    def __iter__(self):
        return iter(self._map)

    def get(self, key, default=None):
        return self._map.get(key, default)

    def __contains__(self, key):
        return key in self._map

    def __getitem__(self, key):
        return self._map[key]

    def keys(self):
        return self._map.keys()

    def preload(self):
        """Loads the underlying data, if it's not already loaded"""
        self._map

    def addfile(self, f, oldstate, state, mode, size, mtime):
        """Add a tracked file to the dirstate."""
        if oldstate in "?r" and "_dirs" in self.__dict__:
            self._dirs.addpath(f)
        if oldstate == "?" and "_alldirs" in self.__dict__:
            self._alldirs.addpath(f)
        self._map[f] = dirstatetuple(state, mode, size, mtime)
        if state != 'n' or mtime == -1:
            self.nonnormalset.add(f)
        if size == -2:
            self.otherparentset.add(f)

    def removefile(self, f, oldstate, size):
        """
        Mark a file as removed in the dirstate.

        The `size` parameter is used to store sentinel values that indicate
        the file's previous state.  In the future, we should refactor this
        to be more explicit about what that state is.
        """
        if oldstate not in "?r" and "_dirs" in self.__dict__:
            self._dirs.delpath(f)
        if oldstate == "?" and "_alldirs" in self.__dict__:
            self._alldirs.addpath(f)
        if "filefoldmap" in self.__dict__:
            normed = util.normcase(f)
            self.filefoldmap.pop(normed, None)
        self._map[f] = dirstatetuple('r', 0, size, 0)
        self.nonnormalset.add(f)

    def dropfile(self, f, oldstate):
        """
        Remove a file from the dirstate.  Returns True if the file was
        previously recorded.
        """
        exists = self._map.pop(f, None) is not None
        if exists:
            if oldstate != "r" and "_dirs" in self.__dict__:
                self._dirs.delpath(f)
            if "_alldirs" in self.__dict__:
                self._alldirs.delpath(f)
        if "filefoldmap" in self.__dict__:
            normed = util.normcase(f)
            self.filefoldmap.pop(normed, None)
        self.nonnormalset.discard(f)
        return exists

    def clearambiguoustimes(self, files, now):
        for f in files:
            e = self.get(f)
            if e is not None and e[0] == 'n' and e[3] == now:
                self._map[f] = dirstatetuple(e[0], e[1], e[2], -1)
                self.nonnormalset.add(f)

    def nonnormalentries(self):
        '''Compute the nonnormal dirstate entries from the dmap'''
        try:
            return parsers.nonnormalotherparententries(self._map)
        except AttributeError:
            nonnorm = set()
            otherparent = set()
            for fname, e in self._map.iteritems():
                if e[0] != 'n' or e[3] == -1:
                    nonnorm.add(fname)
                if e[0] == 'n' and e[2] == -2:
                    otherparent.add(fname)
            return nonnorm, otherparent

    @propertycache
    def filefoldmap(self):
        """Returns a dictionary mapping normalized case paths to their
        non-normalized versions.
        """
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

    def hastrackeddir(self, d):
        """
        Returns True if the dirstate contains a tracked (not removed) file
        in this directory.
        """
        return d in self._dirs

    def hasdir(self, d):
        """
        Returns True if the dirstate contains a file (tracked or removed)
        in this directory.
        """
        return d in self._alldirs

    @propertycache
    def _dirs(self):
        return util.dirs(self._map, 'r')

    @propertycache
    def _alldirs(self):
        return util.dirs(self._map)

    def _opendirstatefile(self):
        fp, mode = txnutil.trypending(self._root, self._opener, self._filename)
        if self._pendingmode is not None and self._pendingmode != mode:
            fp.close()
            raise error.Abort(_('working directory state may be '
                                'changed parallelly'))
        self._pendingmode = mode
        return fp

    def parents(self):
        if not self._parents:
            try:
                fp = self._opendirstatefile()
                st = fp.read(40)
                fp.close()
            except IOError as err:
                if err.errno != errno.ENOENT:
                    raise
                # File doesn't exist, so the current state is empty
                st = ''

            l = len(st)
            if l == 40:
                self._parents = st[:20], st[20:40]
            elif l == 0:
                self._parents = [nullid, nullid]
            else:
                raise error.Abort(_('working directory state appears '
                                    'damaged!'))

        return self._parents

    def setparents(self, p1, p2):
        self._parents = (p1, p2)
        self._dirtyparents = True

    def read(self):
        # ignore HG_PENDING because identity is used only for writing
        self.identity = util.filestat.frompath(
            self._opener.join(self._filename))

        try:
            fp = self._opendirstatefile()
            try:
                st = fp.read()
            finally:
                fp.close()
        except IOError as err:
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
        p = parse_dirstate(self._map, self.copymap, st)
        if not self._dirtyparents:
            self.setparents(*p)

        # Avoid excess attribute lookups by fast pathing certain checks
        self.__contains__ = self._map.__contains__
        self.__getitem__ = self._map.__getitem__
        self.get = self._map.get

    def write(self, st, now):
        st.write(parsers.pack_dirstate(self._map, self.copymap,
                                       self.parents(), now))
        st.close()
        self._dirtyparents = False
        self.nonnormalset, self.otherparentset = self.nonnormalentries()

    @propertycache
    def nonnormalset(self):
        nonnorm, otherparents = self.nonnormalentries()
        self.otherparentset = otherparents
        return nonnorm

    @propertycache
    def otherparentset(self):
        nonnorm, otherparents = self.nonnormalentries()
        self.nonnormalset = nonnorm
        return otherparents

    @propertycache
    def identity(self):
        self._map
        return self.identity

    @propertycache
    def dirfoldmap(self):
        f = {}
        normcase = util.normcase
        for name in self._dirs:
            f[normcase(name)] = name
        return f
